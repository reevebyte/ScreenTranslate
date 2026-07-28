"""把 OCR 出来的「行」合并成「段落块」。

为什么要合并：逐行送去翻译会把一句话拆断，译文质量很差。
合并成段落后整段翻译，再把译文重新排回段落的矩形区域里。
"""
from __future__ import annotations

import statistics
from dataclasses import dataclass, field

from .ocr.base import Line


def _is_cjk(ch: str) -> bool:
    o = ord(ch)
    return 0x3000 <= o <= 0x30FF or 0x3400 <= o <= 0x9FFF or 0xAC00 <= o <= 0xD7AF or 0xFF00 <= o <= 0xFFEF


@dataclass
class Block:
    lines: list[Line] = field(default_factory=list)
    # ``None`` 表示沿用 OCR 原文；空字符串也是有效的人工覆盖值。
    # 校对结果放在 Block 本身，后续重译、重绘就不会又退回错误的 OCR 文本。
    edited_text: str | None = None

    @property
    def confidence(self) -> float | None:
        """Character-weighted confidence from lines that expose engine data."""
        known = [
            (line.confidence, max(1, sum(not ch.isspace() for ch in line.text)))
            for line in self.lines
            if line.confidence is not None
        ]
        if not known:
            return None
        total_weight = sum(weight for _, weight in known)
        return sum(confidence * weight for confidence, weight in known) / total_weight

    @property
    def x(self) -> float:
        return min(l.x for l in self.lines)

    @property
    def y(self) -> float:
        return min(l.y for l in self.lines)

    @property
    def right(self) -> float:
        return max(l.right for l in self.lines)

    @property
    def bottom(self) -> float:
        return max(l.bottom for l in self.lines)

    @property
    def w(self) -> float:
        return self.right - self.x

    @property
    def h(self) -> float:
        return self.bottom - self.y

    @property
    def line_height(self) -> float:
        return statistics.median(l.h for l in self.lines)

    @property
    def recognized_text(self) -> str:
        """把 OCR 多行拼成一段，不包含人工校对覆盖。"""
        out = ""
        for line in self.lines:
            t = line.text.strip()
            if not t:
                continue
            if out:
                if _is_cjk(out[-1]) or _is_cjk(t[0]):
                    pass  # 中文换行处不该出现空格
                elif out.endswith("-"):
                    out = out[:-1]  # 英文断词连字符，直接接上
                else:
                    out += " "
            out += t
        return out

    @property
    def text(self) -> str:
        """当前生效文本：优先使用人工校对，未校对时使用 OCR 原文。"""
        return self.recognized_text if self.edited_text is None else self.edited_text

    @property
    def centered(self) -> bool:
        """判断原文是否居中排版，译文沿用同样的对齐方式看起来才不突兀。

        判据是「各行中心点比各行左边缘更整齐」——左对齐的段落哪怕每行长度接近，
        左边缘也总是严丝合缝的，不会被误判成居中。
        """
        if len(self.lines) < 2:
            return False
        lefts = [l.x for l in self.lines]
        centers = [l.cx for l in self.lines]
        spread_left = max(lefts) - min(lefts)
        spread_center = max(centers) - min(centers)
        # 左边缘本来就参差不齐，且中心点明显更集中，才认为是居中排版
        return spread_left > self.w * 0.08 and spread_center * 2 < spread_left


def _observed_pitch(block: Block) -> float | None:
    """段落里已经确立的行距。

    取「最小值」而不是中位数是有意为之：一旦某次合并判错，中位数会被拉大，
    下一次的阈值跟着变松，于是错误连锁放大——项目符号列表整个被并成一段就是这么来的。
    用最小值，阈值只会越来越严。
    """
    ls = block.lines
    if len(ls) < 2:
        return None
    return min(ls[i + 1].y - ls[i].y for i in range(len(ls) - 1))


def _pitch_threshold(lines: list[Line]) -> float | None:
    """从**这一屏自己的行距分布**里找出「段内换行」和「段间空开」的分界。

    写死一个倍数（行高 × 1.9）是不够的：排版密一点的时候，段间距也会落进阈值以内，
    两段就被并成一段。而这两种行距本来是两簇——段内一簇小的，段间一簇大的，
    中间有个明显的谷。用 Otsu 的思路找那个谷：
    枚举切分点，取「两边组内方差之和」最小的那个。

    分不出两簇（整篇匀速行距的一段、或者一项一行的列表）就返回 None，
    交回给原来那套基于行高的规则去判断——那种情况下行距本来就没有信息量。
    """
    pitches: list[float] = []
    for a, b in zip(lines, lines[1:]):
        pitch = b.y - a.y
        if pitch <= 0:
            continue
        ox = min(a.right, b.right) - max(a.x, b.x)
        if ox > 0 and ox / max(1.0, min(a.w, b.w)) >= 0.35:   # 同一栏才算
            pitches.append(pitch)
    if len(pitches) < 4:
        return None

    xs = sorted(pitches)
    best_var, cut = None, None
    for i in range(1, len(xs)):
        lo, hi = xs[:i], xs[i:]
        var = len(lo) * statistics.pvariance(lo) + len(hi) * statistics.pvariance(hi)
        if best_var is None or var < best_var:
            best_var, cut = var, (xs[i - 1] + xs[i]) / 2.0
    if cut is None:
        return None

    lo = [x for x in xs if x <= cut]
    hi = [x for x in xs if x > cut]
    # 「常规行距」那一簇要有足够样本，两簇之间也要真的拉开，否则不能当分界用
    if len(lo) < 3 or not hi:
        return None
    if statistics.mean(hi) < statistics.mean(lo) * 1.25:
        return None
    return cut


def _column_rights(lines: list[Line]) -> dict[int, float]:
    """每一行所在「文字栏」的右边界。

    用来判断一行是不是「排满了」：只有排满的行才可能有续行。
    一行明显没排到头就结束，说明它本身就是完整的一条（列表项、标题、按钮），
    下面那行是新的一条，不该并进来。

    取 85 分位数而不是最大值：同一栏里常常混着一个特别宽的大标题，
    用最大值的话整栏的基准都被它抬高，正文每一行都会被误判成「没排满」。
    """
    out: dict[int, float] = {}
    for a in lines:
        peers = [a.right]
        for b in lines:
            if b is a:
                continue
            ox = min(a.right, b.right) - max(a.x, b.x)
            if ox > 0 and ox / max(1.0, min(a.w, b.w)) >= 0.5:
                peers.append(b.right)
        peers.sort()
        out[id(a)] = peers[min(len(peers) - 1, int(round((len(peers) - 1) * 0.85)))]
    return out


def group_lines(lines: list[Line], pitch_ratio: float = 1.9) -> list[Block]:
    """按行距 + 排版宽度把行聚成段落。

    这里用的是「行距」（相邻两行顶端的距离）而不是「行间空隙」：
    OCR 给的行高取决于这一行有没有 y、g 这种带降部的字母，同一段里能差三成，
    拿它推空隙会把好好的段落判散；行距则稳定得多。
    """
    lines = [l for l in lines if l.text.strip()]
    if not lines:
        return []
    lines.sort(key=lambda l: (round(l.y / 4), l.x))
    col_right = _column_rights(lines)
    cut = _pitch_threshold(lines)

    blocks: list[Block] = []
    for line in lines:
        placed = False
        for block in reversed(blocks):
            last = block.lines[-1]
            pitch = line.y - last.y
            ref_h = max(last.h, line.h)

            # 1) 纵向距离要合理（轻微负值是同一行被切成两块，允许）
            if pitch < -ref_h * 0.4:
                continue
            observed = _observed_pitch(block)
            limit = observed * 1.28 + 2 if observed else ref_h * pitch_ratio
            if cut is not None:
                # 这一屏自己的行距分布给出了分界，就以它为准。
                # 取 min 而不是直接替换：这条只会让切分**更容易**，
                # 不会放松前面那些「别乱并」的规则。
                limit = min(limit, cut)
            if pitch > limit:
                continue
            # 2) 上一行没排到栏宽就结束了，说明它自成一条，下面是新的一条。
            #    只在段落还只有一行时查这条：一旦行距已经确立，行距本身就够严了，
            #    再查「排满」会把段落的最后一行（本来就短）挡在外面。
            if observed is None and last.right < col_right[id(last)] - ref_h * 2.5:
                continue
            # 3) 字号不能差太多，避免把标题和正文并进同一段
            if not (0.45 <= line.h / max(last.h, 1e-6) <= 2.0):
                continue
            # 4) 横向要有实质重叠，避免把并排的两栏合成一段
            ox = min(line.right, block.right) - max(line.x, block.x)
            if ox <= 0:
                continue
            if ox / max(1.0, min(line.w, block.w)) < 0.35:
                continue

            block.lines.append(line)
            placed = True
            break
        if not placed:
            blocks.append(Block([line]))

    blocks = _split_leading_headings(blocks)
    blocks = _split_non_paragraphs(blocks)
    blocks.sort(key=lambda b: (b.y, b.x))
    return blocks


def _clipped_by_edge(block: Block) -> bool:
    """这些行是不是被某条硬边界（面板边缘、滚动区右侧）**切掉**的。

    被切掉的列表项和排满的正文长得一模一样——右边缘都顶到同一个 x，
    「每行都排满」那条判据完全分不出来（侧栏最近会话列表就是这么整段并掉的）。

    真正的区别在于**贴得有多齐**：
      · 正文换行是在词的边界断的，每行末尾差一个词宽，右边缘天然参差十几到几十像素；
      · 被切掉的行是在同一个像素上被裁断的，右边缘严丝合缝到 1~2 像素。
    而两端对齐（justify）的正文虽然也严丝合缝，但它是**每一行**都对齐；
    被切的列表里总还有几条本来就短、够不到那条边。
    所以「一部分像素级贴边 + 另有明显够不到的」= 被切了，不是段落。
    """
    interior = block.lines[:-1]        # 最后一行本来就该短，不参与
    if len(interior) < 3:
        return False
    edge = max(l.right for l in interior)
    flush = sum(1 for l in interior if l.right >= edge - 2.0)
    short = sum(1 for l in interior if l.right < edge - max(8.0, block.w * 0.06))
    return flush >= 3 and short >= 1


def _looks_like_paragraph(block: Block, ratio: float = 0.7, tol_frac: float = 0.12) -> bool:
    """检查一个多行块到底是不是「一段换行的文字」。

    判据：真正的段落除了最后一行，每一行都是被排版挤满的，右边缘几乎对齐；
    而列表、菜单、表格那种「一项一行」的东西，每行宽度是随机的。

    这一条是兜底。行距信号在密排列表上会完全失灵——比如四栏国家名单，
    行距均匀得和段落一样（每行都是 28px），单看行距根本分不出来，
    结果整栏几十项被并成一段，背景填充再把它们全抹掉。
    """
    lines = block.lines
    if len(lines) < 3:
        return True  # 只有两行时证据不足，交给前面的行距/排满规则判断

    # 被边缘切掉的列表会把「排满」判据骗过去，先单独拦一道
    if _clipped_by_edge(block):
        return False

    right = max(l.right for l in lines)
    left = min(l.x for l in lines)
    tol = max(4.0, (right - left) * tol_frac)
    interior = lines[:-1]          # 最后一行本来就该短，不参与统计
    full = sum(1 for l in interior if l.right >= right - tol)
    return full / len(interior) >= ratio


def _split_non_paragraphs(blocks: list[Block]) -> list[Block]:
    out: list[Block] = []
    for block in blocks:
        if _looks_like_paragraph(block):
            out.append(block)
        else:
            out.extend(Block([l]) for l in block.lines)
    return out


def _split_leading_headings(blocks: list[Block]) -> list[Block]:
    """把误并进段落的小标题拆出来单独成块。

    标题和正文字号往往只差一两个像素，靠字号阈值分不开；
    但标题几乎总是「明显更短 + 略高 / 下方间距更大」，用这组特征来拆更稳。
    """
    out: list[Block] = []
    for block in blocks:
        lines = block.lines
        if len(lines) < 2:
            out.append(block)
            continue

        head, rest = lines[0], lines[1:]
        rest_h = statistics.median(l.h for l in rest)
        rest_w = max(l.w for l in rest)

        # 标题必须明显短于正文，否则可能只是一个恰好偏短的首行
        if head.w > rest_w * 0.55:
            out.append(block)
            continue

        taller = head.h > rest_h * 1.12
        looser = False
        if len(rest) >= 2:
            inner = statistics.median(
                rest[i + 1].y - rest[i].bottom for i in range(len(rest) - 1)
            )
            looser = (rest[0].y - head.bottom) > inner * 1.5 + 1.5

        if taller or looser:
            out.append(Block([head]))
            out.append(Block(rest))
        else:
            out.append(block)
    return out
