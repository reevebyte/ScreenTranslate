"""把译文重新画回原文所在的位置。

做法和 Google Lens 类似：
  1. 从原图里采样每个段落的背景色和文字色；
  2. 用背景色把原文盖掉；
  3. 用采样到的文字色，把译文按自适应字号重排进同一块区域。
这样译文看起来像是原本就长在那儿的，而不是一个飘在上面的框。
"""
from __future__ import annotations

from array import array
from dataclasses import dataclass
from math import ceil

from PIL import Image, ImageFilter, ImageMath, ImageStat
from PySide6.QtCore import QRect, QRectF, Qt
from PySide6.QtGui import (QColor, QFont, QFontDatabase, QFontMetrics, QImage,
                           QPainter)

from .layout import Block


@dataclass
class TextLayout:
    """一段译文最终画在哪、用什么字体画。

    结果窗口拿它在同样的位置铺一层透明的可选中文本，
    这样译文既是「画在图上的」（贴合原版式），又能用鼠标划选复制。
    """

    rect: QRect      # 物理像素
    text: str
    font: QFont      # pixelSize 单位是物理像素
    align: int

# 段落之间留一点呼吸空间，盖住原文时也顺带盖掉抗锯齿的毛边
PAD_X = 2
PAD_Y = 1
# 译文放不下时最多把原区域撑到几倍高
MAX_GROW = 2.5
# 也允许往右借一点：原文框的宽度只是**原文那几个字**的宽度，
# 右边往往还空着一大片（按钮、菜单项里尤其明显）。
# 不借的话，「Settings」那种 64px 宽的框要塞下「设置与偏好配置项总览」，
# 只能压到最小字号再折成两行，又小又挤。
MAX_GROW_X = 2.4
# 字号最多允许比「跟原文一样大」小到这个比例；再小就改成往下多占一行。
# 单行标签宁可挤一挤也不想换行，但挤到看不清就没意义了。
SHRINK_FLOOR = 0.8


def pil_to_qimage(img: Image.Image) -> QImage:
    rgb = img if img.mode == "RGB" else img.convert("RGB")
    data = rgb.tobytes("raw", "RGB")
    return QImage(data, rgb.width, rgb.height, rgb.width * 3, QImage.Format.Format_RGB888).copy()


def qimage_to_pil(img: QImage) -> Image.Image:
    """Copy a QImage into a tightly owned Pillow RGB image."""
    rgb = img.convertToFormat(QImage.Format.Format_RGB888)
    return Image.frombytes(
        "RGB",
        (rgb.width(), rgb.height()),
        bytes(rgb.constBits()),
        "raw",
        "RGB",
        rgb.bytesPerLine(),
        1,
    )


# ------------------------------------------------------------------ 取色

def _luminance(c: tuple[int, int, int]) -> float:
    out = []
    for v in c:
        v = v / 255.0
        out.append(v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4)
    return 0.2126 * out[0] + 0.7152 * out[1] + 0.0722 * out[2]


def _contrast(a, b) -> float:
    la, lb = _luminance(a), _luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


@dataclass(frozen=True)
class _RGBPixels:
    width: int
    height: int
    data: bytes
    image: Image.Image | None = None


_FIVE_BIT_LUT = [value >> 3 for value in range(256)]
_BINARY_MASK_LUT = [0] + [255] * 255


def _binary_mask(image: Image.Image) -> Image.Image:
    """Convert ImageMath's integer 0/1 result to an 8-bit Pillow mask."""
    return image.convert("L").point(_BINARY_MASK_LUT)


def _mode_color_image(crop: Image.Image) -> tuple[float, float, float]:
    """C-backed equivalent of `_mode_color` for a Pillow RGB crop."""
    red, green, blue = (channel.point(_FIVE_BIT_LUT) for channel in crop.split())
    keys = ImageMath.lambda_eval(
        lambda args: args["red"] * 1024 + args["green"] * 32 + args["blue"],
        red=red,
        green=green,
        blue=blue,
    )
    colors = keys.getcolors(maxcolors=32768) or []
    if not colors:
        return 255.0, 255.0, 255.0
    winner_count = max(count for count, _value in colors)
    # The old 32768-entry array kept the first bucket on ties.
    winner = min(value for count, value in colors if count == winner_count)
    mask = _binary_mask(
        ImageMath.lambda_eval(lambda args: args["keys"] == winner, keys=keys)
    )
    return tuple(ImageStat.Stat(crop, mask=mask).mean)


def _foreground_color_image(
    crop: Image.Image, bg: tuple[float, float, float]
) -> tuple[float, float, float] | None:
    """Find the farthest-color percentile without Python pixel iteration."""
    # Ring medians are integers or half-integers. Scaling by two therefore
    # preserves the old squared Euclidean ordering exactly; mode backgrounds
    # are rounded to the nearest half, which randomized regression checks show
    # does not alter the final rounded QColor.
    bg2 = tuple(int(round(value * 2)) for value in bg)
    red, green, blue = crop.split()
    distances = ImageMath.lambda_eval(
        lambda args: (
            (args["red"] * 2 - bg2[0]) * (args["red"] * 2 - bg2[0])
            + (args["green"] * 2 - bg2[1]) * (args["green"] * 2 - bg2[1])
            + (args["blue"] * 2 - bg2[2]) * (args["blue"] * 2 - bg2[2])
        ),
        red=red,
        green=green,
        blue=blue,
    )
    # Distances were multiplied by four, so 60**2 becomes 14400.
    colors = distances.getcolors(maxcolors=780301) or []
    far = sorted((value, count) for count, value in colors if value > 14400)
    count = sum(amount for _value, amount in far)
    crop_count = crop.width * crop.height
    if count < max(8, int(crop_count * 0.01)):
        return None

    target = ceil((count - 1) * 0.6)
    seen = 0
    cutoff = far[-1][0]
    for value, amount in far:
        seen += amount
        if seen > target:
            cutoff = value
            break
    mask = _binary_mask(
        ImageMath.lambda_eval(
            lambda args: args["distances"] >= cutoff,
            distances=distances,
        )
    )
    return tuple(ImageStat.Stat(crop, mask=mask).mean)


def _histogram_median(histogram: list[int], count: int) -> float:
    """Return NumPy-compatible channel median from a 256-bin histogram."""
    lower_rank = (count - 1) // 2
    upper_rank = count // 2
    lower: int | None = None
    upper = 0
    seen = 0
    for value, amount in enumerate(histogram):
        seen += amount
        if seen > lower_rank and lower is None:
            lower = value
        if seen > upper_rank:
            upper = value
            break
    return ((lower or 0) + upper) / 2.0


def _mode_color(arr: _RGBPixels, x0: int, y0: int, x1: int, y1: int) -> tuple[float, float, float]:
    """Mean of the first most-populated 5-bit RGB bucket."""
    counts = array("I", [0]) * 32768
    data = arr.data
    stride = arr.width * 3
    for y in range(y0, y1):
        pos = y * stride + x0 * 3
        end = y * stride + x1 * 3
        while pos < end:
            key = ((data[pos] >> 3) << 10) | ((data[pos + 1] >> 3) << 5) | (data[pos + 2] >> 3)
            counts[key] += 1
            pos += 3

    winner = 0
    winner_count = 0
    for key, count in enumerate(counts):
        if count > winner_count:
            winner = key
            winner_count = count

    red = green = blue = total = 0
    for y in range(y0, y1):
        pos = y * stride + x0 * 3
        end = y * stride + x1 * 3
        while pos < end:
            if (((data[pos] >> 3) << 10) | ((data[pos + 1] >> 3) << 5) | (data[pos + 2] >> 3)) == winner:
                red += data[pos]
                green += data[pos + 1]
                blue += data[pos + 2]
                total += 1
            pos += 3
    if not total:
        return 255.0, 255.0, 255.0
    return red / total, green / total, blue / total


def _select_kth(values: array, index: int) -> float:
    """In-place quickselect without expanding pixel distances into Python objects."""
    left = 0
    right = len(values) - 1
    while left < right:
        pivot = values[(left + right) // 2]
        low, high = left, right
        while low <= high:
            while low <= right and values[low] < pivot:
                low += 1
            while high >= left and values[high] > pivot:
                high -= 1
            if low <= high:
                values[low], values[high] = values[high], values[low]
                low += 1
                high -= 1
        if index <= high:
            right = high
        elif index >= low:
            left = low
        else:
            break
    return float(values[index])


def sample_colors(arr: _RGBPixels, rect: QRect, line_h: float = 0.0) -> tuple[QColor, QColor]:
    """返回 (背景色, 文字色)。

    背景色优先从**文字框外面一圈**取：框里面如果是大号粗体字，文字像素可能比背景还多；
    更要命的是渐变背景会被量化打散成一堆颜色桶，而纯色的文字是集中的一桶，
    于是「取众数」会把文字色当成背景色，前景背景整个颠倒。框外那一圈必定没有文字，稳得多。

    文字色取框内离背景最远的那批像素的均值；对比度不够就退回黑或白。
    """
    h, w = arr.height, arr.width
    x0 = max(0, rect.left())
    y0 = max(0, rect.top())
    x1 = min(w, rect.right() + 1)
    y1 = min(h, rect.bottom() + 1)
    if x1 - x0 < 2 or y1 - y0 < 2:
        return QColor(255, 255, 255), QColor(0, 0, 0)

    pad = int(max(2, round((line_h or (y1 - y0)) * 0.35)))
    ox0, oy0 = max(0, x0 - pad), max(0, y0 - pad)
    ox1, oy1 = min(w, x1 + pad), min(h, y1 + pad)
    crop_count = (x1 - x0) * (y1 - y0)
    source = arr.image
    if source is not None:
        crop = source.crop((x0, y0, x1, y1))
        outer = source.crop((ox0, oy0, ox1, oy1))
        ring_count = outer.width * outer.height - crop_count
        if ring_count >= 24:
            outer_histogram = outer.histogram()
            crop_histogram = crop.histogram()
            histograms = [
                [
                    outer_histogram[channel * 256 + value]
                    - crop_histogram[channel * 256 + value]
                    for value in range(256)
                ]
                for channel in range(3)
            ]
            bg = tuple(
                _histogram_median(histogram, ring_count)
                for histogram in histograms
            )
        else:
            bg = _mode_color_image(crop)
        fg = _foreground_color_image(crop, bg)
        if fg is None:
            fg = (0, 0, 0) if _luminance(bg) > 0.5 else (255, 255, 255)
    else:
        # Compatibility path for callers that only provide a byte buffer.
        data = arr.data
        stride = w * 3
        histograms = [[0] * 256 for _ in range(3)]
        ring_count = 0
        for y in range(oy0, oy1):
            pos = y * stride + ox0 * 3
            end = y * stride + ox1 * 3
            while pos < end:
                x = (pos - y * stride) // 3
                if not (x0 <= x < x1 and y0 <= y < y1):
                    histograms[0][data[pos]] += 1
                    histograms[1][data[pos + 1]] += 1
                    histograms[2][data[pos + 2]] += 1
                    ring_count += 1
                pos += 3

        if ring_count >= 24:
            bg = tuple(
                _histogram_median(histogram, ring_count)
                for histogram in histograms
            )
        else:
            bg = _mode_color(arr, x0, y0, x1, y1)

        distances = array("d")
        for y in range(y0, y1):
            pos = y * stride + x0 * 3
            end = y * stride + x1 * 3
            while pos < end:
                distance = (
                    (data[pos] - bg[0]) ** 2
                    + (data[pos + 1] - bg[1]) ** 2
                    + (data[pos + 2] - bg[2]) ** 2
                )
                if distance > 60 * 60:
                    distances.append(distance)
                pos += 3

        if len(distances) >= max(8, int(crop_count * 0.01)):
            cutoff = _select_kth(distances, ceil((len(distances) - 1) * 0.6))
            red = green = blue = total = 0
            for y in range(y0, y1):
                pos = y * stride + x0 * 3
                end = y * stride + x1 * 3
                while pos < end:
                    distance = (
                        (data[pos] - bg[0]) ** 2
                        + (data[pos + 1] - bg[1]) ** 2
                        + (data[pos + 2] - bg[2]) ** 2
                    )
                    if distance >= cutoff:
                        red += data[pos]
                        green += data[pos + 1]
                        blue += data[pos + 2]
                        total += 1
                    pos += 3
            fg = (red / total, green / total, blue / total) if total else bg
        else:
            fg = (0, 0, 0) if _luminance(bg) > 0.5 else (255, 255, 255)

    bg_t = tuple(int(round(v)) for v in bg)
    fg_t = tuple(int(round(v)) for v in fg)
    if _contrast(bg_t, fg_t) < 3.0:
        fg_t = (17, 17, 17) if _luminance(bg_t) > 0.45 else (240, 240, 240)
    return QColor(*bg_t), QColor(*fg_t)


# ------------------------------------------------------------------ 抹字

def erase_text(crop: Image.Image, text_is_dark: bool, line_h: float) -> Image.Image | None:
    """把一小块图里的文字抹掉，只留背景。

    为什么不用纯色填充：译文往往比原文短，纯色块会在译文右边露出一截，
    背景只要是渐变、图片或者带边框，那截色块就特别扎眼（就是所谓的「白斑」）。

    这里用形态学滤波：深色字就取邻域最大值（亮的背景把笔画吞掉），
    浅色字反过来取最小值。滤波窗口比笔画粗一点就能抹干净，
    而背景的渐变、纹理会被原样保留下来。
    """
    if crop.width < 3 or crop.height < 3:
        return None
    # 笔画粗细大致是行高的 1/8，窗口要能跨过一整笔
    size = int(round(max(3.0, min(15.0, line_h * 0.34))))
    if size % 2 == 0:
        size += 1
    try:
        f = ImageFilter.MaxFilter if text_is_dark else ImageFilter.MinFilter
        # A square max/min filter is separable by repeated 3x3 morphology:
        # radius r applied once is pixel-identical to r passes at radius 1.
        # Pillow's generic 15x15 rank filter is roughly five times slower.
        out = crop
        small = f(3)
        for _ in range(size // 2):
            out = out.filter(small)
        # 滤波会让背景边界发硬，轻微模糊一下更像原来的样子
        return out.filter(ImageFilter.GaussianBlur(max(0.6, size * 0.12)))
    except ValueError:
        return None


# ------------------------------------------------------------------ 排版

def _fit_font(text: str, rect: QRect, family: str, start_px: int, min_px: int,
              bold: bool) -> tuple[QFont, int]:
    """从 start_px 往下找第一个能塞进 rect 的字号。返回 (字体, 实际需要的高度)。"""
    font = QFont(family)
    font.setBold(bold)
    last_h = rect.height()
    for size in range(max(start_px, min_px), min_px - 1, -1):
        font.setPixelSize(size)
        fm = QFontMetrics(font)
        need = fm.boundingRect(
            QRect(0, 0, rect.width(), 100000),
            int(Qt.TextFlag.TextWordWrap),
            text,
        )
        last_h = need.height()
        if need.height() <= rect.height() and need.width() <= rect.width():
            return font, need.height()
    font.setPixelSize(min_px)
    return font, last_h


def _grow_limit(block_rect: QRect, others: list[QRect], img_h: int) -> int:
    """算出这个段落最多能往下长到哪儿——不要压到下面那段的头上。"""
    limit = img_h
    for other in others:
        if other.top() <= block_rect.top():
            continue
        if other.right() < block_rect.left() or other.left() > block_rect.right():
            continue  # 不在同一竖直通道里，不构成阻挡
        limit = min(limit, other.top() - 1)
    return max(block_rect.bottom() + 1, limit)


def _grow_limit_up(block_rect: QRect, others: list[QRect]) -> int:
    """算出这个段落最多能往上长到哪儿——不要压到上面那段的脚下。"""
    limit = 0
    for other in others:
        if other.bottom() >= block_rect.bottom():
            continue
        if other.right() < block_rect.left() or other.left() > block_rect.right():
            continue
        limit = max(limit, other.bottom() + 1)
    return min(block_rect.top(), limit) if limit else 0


def _grow_limit_x(block_rect: QRect, others: list[QRect], img_w: int) -> int:
    """算出这个段落最多能往右长到哪儿——不要压到右边那段身上。"""
    limit = img_w
    for other in others:
        if other.left() <= block_rect.left():
            continue
        if other.bottom() < block_rect.top() or other.top() > block_rect.bottom():
            continue  # 不在同一横向通道里
        limit = min(limit, other.left() - 1)
    return max(block_rect.right() + 1, limit)


def _bg_run(arr: _RGBPixels, rect: QRect, bg: QColor, limit: int, down: bool) -> int:
    """从 rect 的下（上）边缘往外走，同一片背景色最多铺到哪一行。

    `_grow_limit` 只知道别的**文字块**在哪，不知道按钮、面板、代码块的边界在哪——
    译文往下长过了头，就会掉到人家的底色外面，成了悬在半空的字。
    这里逐行数「这一行还有多少像素是本段的背景色」，第一次明显不是了就停在那儿。
    背景色是从框外一圈采的，本来就是这块底色，比按框高的倍数拍脑袋靠谱。

    返回一个绝对 y 坐标：往下时是最后一行仍属于本片背景的行号，往上时是第一行。
    """
    h, w = arr.height, arr.width
    x0, x1 = max(0, rect.left()), min(w, rect.right() + 1)
    if x1 - x0 < 4:
        return limit
    if down:
        y0, y1 = min(h, rect.bottom() + 1), min(h, limit + 1)
    else:
        y0, y1 = max(0, limit), max(0, rect.top())
    if y1 - y0 < 1:
        return limit

    # 容差给得松一点：渐变底、纹理底、抗锯齿的毛边都不该算「换了块地方」，
    # 真正要拦的是边框和另一块颜色明显不同的面板。
    bg_red, bg_green, bg_blue = bg.red(), bg.green(), bg.blue()
    if arr.image is not None:
        strip = arr.image.crop((x0, y0, x1, y1))
        red, green, blue = strip.split()
        matches = ImageMath.lambda_eval(
            lambda args: (
                (args["red"] >= max(0, bg_red - 26))
                & (args["red"] <= min(255, bg_red + 26))
                & (args["green"] >= max(0, bg_green - 26))
                & (args["green"] <= min(255, bg_green + 26))
                & (args["blue"] >= max(0, bg_blue - 26))
                & (args["blue"] <= min(255, bg_blue + 26))
            ),
            red=red,
            green=green,
            blue=blue,
        )
        match_bytes = _binary_mask(matches).tobytes()
        width = x1 - x0
        rows = range(y1 - y0) if down else range(y1 - y0 - 1, -1, -1)
        for relative_y in rows:
            start = relative_y * width
            matched = match_bytes.count(255, start, start + width)
            if matched * 4 < width * 3:
                y = y0 + relative_y
                return y - 1 if down else y + 1
        return limit

    data = arr.data
    stride = w * 3
    rows = range(y0, y1) if down else range(y1 - 1, y0 - 1, -1)
    for y in rows:
        matched = 0
        pos = y * stride + x0 * 3
        end = y * stride + x1 * 3
        while pos < end:
            if (
                abs(data[pos] - bg_red) <= 26
                and abs(data[pos + 1] - bg_green) <= 26
                and abs(data[pos + 2] - bg_blue) <= 26
            ):
                matched += 1
            pos += 3
        if matched * 4 < (x1 - x0) * 3:
            return y - 1 if down else y + 1
    return limit


def _in_text_flow(rect: QRect, line_h: float, others: list[QRect],
                  heights: list[float]) -> bool:
    """这一行是「正文里的一行」，还是按钮、菜单项那种孤立的标签？

    只看行数分不出来：一段正文的最后一行、一个短句子，都只有一行。
    但两者往下长的代价完全不同——正文多占一行毫不违和，按钮多占一行就掉出底色了。

    判据是「上下有没有同伙」：正文的行总是成群出现在同一条竖直通道里、字号也一样；
    按钮和菜单项要么孤零零，要么邻居的字号对不上。
    """
    for other, h in zip(others, heights):
        if other.right() < rect.left() or other.left() > rect.right():
            continue                       # 不在同一条竖直通道里
        if abs(h - line_h) > line_h * 0.18:
            continue                       # 字号对不上，多半是标题或别的东西
        gap = (other.top() - rect.bottom()) if other.top() > rect.top() else (rect.top() - other.bottom())
        if 0 <= gap <= line_h * 3:         # 挨得够近，是同一段文字流
            return True
    return False


_FALLBACKS = ("Microsoft YaHei UI", "微软雅黑", "Microsoft YaHei",
              "Noto Sans SC", "SimHei", "SimSun")


def covers_chinese(family: str) -> bool:
    """这个字体自己有没有汉字。

    别拿 QFontMetrics.inFont() 判断——它会把「系统替换之后能显示」也算成 True，
    Tahoma、Franklin Gothic 问它都答 True，等于没问。writingSystems() 才是看字体本身。
    """
    try:
        ws = QFontDatabase.writingSystems(family)
    except Exception:
        return True          # 查不了就别拦，宁可放过不可错杀
    return QFontDatabase.WritingSystem.SimplifiedChinese in ws


def resolve_family(family: str) -> str:
    """把配置里的字体名换成一个真能用来画中文的。

    要挡两种情况，两种 Qt 都**不会报错、只会悄悄降级**：

    1. 名字根本不存在（比如字体下拉框有焦点时手滑敲了个字母，
       「Microsoft YaHei UI」变成「Microsoft YaHei UIs」）——Qt 解析成 Tahoma。
    2. 名字存在，但这个字体没有汉字（Franklin Gothic、Segoe UI 之类）——
       中文得靠系统逐字替换，出来的字又丑又不统一，还跟拉丁字母对不齐。

    两种都自己换成中文覆盖好的字体，并且明确说一声。
    """
    families = set(QFontDatabase.families())
    if family in families and covers_chinese(family):
        return family
    why = "系统里没有这个字体" if family not in families else "这个字体没有汉字"
    for cand in _FALLBACKS:
        if cand in families:
            print(f"[render] {family!r}：{why}，改用 {cand!r}")
            return cand
    fallback = next((f for f in sorted(families) if covers_chinese(f)), family)
    print(f"[render] {family!r}：{why}，改用 {fallback!r}")
    return fallback


def size_for_ink(text: str, family: str, bold: bool, target_h: float,
                 min_px: int, max_px: int) -> int:
    """挑一个字号，让译文**画出来有多高**跟原文差不多。

    以前是 `line_h * 0.86`，一个拍脑袋的系数，实测译文只有原文的 0.76~0.91 倍，
    看上去就是小一号。因为两头都在缩：
      · OCR 给的 line_h 本身就是**墨迹高度**，已经比字号小一截；
      · 再乘 0.86 又缩一次；
      · 中文字形自身还有内缩，同样字号下墨迹比字号矮。
    与其叠三层系数去猜，不如直接按「这个字号画出来多高」反推。
    """
    font = QFont(family)
    font.setBold(bold)
    best, best_diff = min_px, None
    for size in range(min_px, max(min_px, max_px) + 1):
        font.setPixelSize(size)
        h = QFontMetrics(font).tightBoundingRect(text).height()
        diff = abs(h - target_h)
        if best_diff is None or diff < best_diff:
            best, best_diff = size, diff
        if h > target_h * 1.4:      # 已经明显偏大了，再往上没意义
            break
    return best


def _needed(text: str, font: QFont, width: int) -> QRect:
    return QFontMetrics(font).boundingRect(
        QRect(0, 0, width, 100000), int(Qt.TextFlag.TextWordWrap), text
    )


def render(
    image: Image.Image,
    blocks: list[Block],
    translations: list[str],
    font_family: str = "Microsoft YaHei UI",
    min_font_px: int = 9,
) -> tuple[QImage, list[TextLayout]]:
    rgb = image if image.mode == "RGB" else image.convert("RGB")
    pixels = rgb.tobytes("raw", "RGB")
    canvas = QImage(
        pixels,
        rgb.width,
        rgb.height,
        rgb.width * 3,
        QImage.Format.Format_RGB888,
    ).copy()
    arr = _RGBPixels(rgb.width, rgb.height, pixels, rgb)
    font_family = resolve_family(font_family)
    layouts: list[TextLayout] = []

    rects = [
        QRect(
            int(b.x) - PAD_X,
            int(b.y) - PAD_Y,
            int(round(b.w)) + PAD_X * 2,
            int(round(b.h)) + PAD_Y * 2,
        )
        for b in blocks
    ]
    line_heights = [b.line_height for b in blocks]

    placed: list[QRect] = []
    painter = QPainter(canvas)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
    try:
        for i, (block, rect) in enumerate(zip(blocks, rects)):
            text = (translations[i] if i < len(translations) else "").strip()
            # 没译或者译文跟原文一模一样（多半是产品名、代码、数字）的，原像素留着不动。
            # 但那块地还是被原文占着的，得记进 placed，不然后面的段落会长到它身上。
            if not text or text == block.text.strip():
                placed.append(rect)
                continue

            line_h = block.line_height
            bg, fg = sample_colors(arr, rect, line_h)
            # 单行短标签常常是粗体，跟着粗一点更贴近原样
            bold = len(block.lines) == 1 and line_h >= 22
            start_px = max(
                min_font_px,
                size_for_ink(text, font_family, bold, line_h,
                             min_font_px, int(line_h * 2) + 4),
            )

            # 前面已经画过的段落，用它们**长完之后**的实际位置来挡；后面还没画的用原框。
            # 不然会两头对着同一条缝各让一半：上一段往下长到下一段的原框顶，
            # 下一段又往上长到上一段的原框底，两个人都以为那块地是空的，最后压在一起。
            others = placed + rects[i + 1:]
            # 先算出这块最多能长到多大：右边和下边各借一点，但都不许压到别的段落头上，
            # 也都不许无限膨胀（中译英通常长 1.5~2 倍，超出太多多半是识别或翻译出了岔子）。
            max_w = min(
                _grow_limit_x(rect, others, canvas.width()) - rect.left(),
                int(rect.width() * MAX_GROW_X),
            )
            # 竖直方向上下都算：多出来的行本来就是上下均摊的，
            # 只量往下能走多远，就会白白少算一半地方。
            down_to = _grow_limit(rect, others, canvas.height())
            down_to = min(down_to, _bg_run(arr, rect, bg, down_to, down=True))
            up_to = _grow_limit_up(rect, others)
            up_to = max(up_to, _bg_run(arr, rect, bg, up_to, down=False))
            max_w = max(rect.width(), max_w)
            max_h = max(rect.height(), min(down_to - up_to + 1,
                                           int(rect.height() * MAX_GROW)))

            # 竖直方向先给多少地方。
            # 单行标签（按钮、菜单项）宁可把字压小一点也别往下长——长出去就掉到按钮底色外面了，
            # 所以只给**一个行盒**。行盒比墨迹高（上下都留了空），拿 OCR 的墨迹高度去卡行盒
            # 会平白压小一号，这里按行盒给。
            # 多行段落不一样：它本来就占好几行，底色是整片的，往下多占一行毫不违和，
            # 而压字号是整段一起变小，一眼就看得出来。中译英普遍要多出 1~2 行，
            # 卡在原高度上就等于逼着它把字号压到 0.7 倍。所以段落直接按能长到的最大高度给。
            probe = QFont(font_family)
            probe.setBold(bold)
            probe.setPixelSize(start_px)
            flowing = len(block.lines) >= 2 or _in_text_flow(
                rect, line_h, others, [h for j, h in enumerate(line_heights) if j != i]
            )
            if flowing:
                fit_h = max_h
            else:
                fit_h = max(rect.height(), min(max_h, QFontMetrics(probe).height()))

            # 在「最大可用范围」里挑字号，而不是死抠原框——有地方就用大一点的字。
            font, _ = _fit_font(text, QRect(rect.left(), rect.top(), max_w, fit_h),
                                font_family, start_px, min_font_px, bold)
            # 单行标签也有个底线：为了不换行把字压到看不清，那就本末倒置了。
            # 掉得太狠就改成允许往下长，宁可多占一行也要保住字号。
            if font.pixelSize() < start_px * SHRINK_FLOOR and max_h > fit_h:
                font, _ = _fit_font(text, QRect(rect.left(), rect.top(), max_w, max_h),
                                    font_family, start_px, min_font_px, bold)

            # 字号定了，再回头看实际需要多大，只占这么大就行，不要白白多抹一片背景
            need = _needed(text, font, max_w)
            draw_rect = QRect(
                rect.left(), rect.top(),
                max(rect.width(), min(max_w, need.width() + PAD_X * 2)),
                max(rect.height(), min(max_h, need.height())),
            )

            # 行盒比原文墨迹高出来的那部分，上下均摊，让字压在原来那条基线附近；
            # 只往下长的话，按钮里的文字会整体沉下去、甚至掉出底色。
            extra = draw_rect.height() - rect.height()
            if extra > 1:
                top = rect.top() - extra // 2
                # 上下都不许越界；地方不够时优先保住上边，宁可从下面挤出来
                draw_rect.moveTop(max(up_to, min(top, down_to - draw_rect.height() + 1)))

            paint_rect = draw_rect.intersected(QRect(0, 0, canvas.width(), canvas.height()))
            if paint_rect.isEmpty():
                placed.append(rect)
                continue
            placed.append(QRect(paint_rect))

            # 先把原文抹掉。优先用「滤波抹字」保住背景的渐变和纹理，
            # 实在不行再退回纯色填充。
            patch = erase_text(
                image.crop(
                    (paint_rect.left(), paint_rect.top(),
                     paint_rect.right() + 1, paint_rect.bottom() + 1)
                ).convert("RGB"),
                text_is_dark=_luminance((fg.red(), fg.green(), fg.blue()))
                < _luminance((bg.red(), bg.green(), bg.blue())),
                line_h=line_h,
            )
            if patch is not None:
                painter.drawImage(paint_rect.topLeft(), pil_to_qimage(patch))
            else:
                painter.setPen(Qt.PenStyle.NoPen)
                painter.setBrush(bg)
                painter.drawRect(QRectF(paint_rect).adjusted(-0.5, -0.5, 0.5, 0.5))

            painter.setFont(font)
            painter.setPen(fg)
            align = Qt.AlignmentFlag.AlignHCenter if block.centered else Qt.AlignmentFlag.AlignLeft
            flags = int(align | Qt.AlignmentFlag.AlignTop | Qt.TextFlag.TextWordWrap)
            painter.drawText(paint_rect, flags, text)
            layouts.append(TextLayout(QRect(paint_rect), text, QFont(font), int(align)))
    finally:
        painter.end()
    return canvas, layouts
