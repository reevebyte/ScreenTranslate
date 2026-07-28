"""基于 Windows 自带 OCR（Windows.Media.Ocr）的识别引擎。

优点：完全离线、免费、几十毫秒出结果、给出逐词包围盒。
缺点：只能识别系统里装了「可选功能 → 光学字符识别」的语言。
"""
from __future__ import annotations

import asyncio
import math
import re
import threading

from PIL import Image

from .base import Line, OcrError, Word

_available_cache: list[str] | None = None
_engine_cache: dict[str, object] = {}
_lock = threading.Lock()


def _winrt_types():
    try:
        from winrt.windows.globalization import Language
        from winrt.windows.graphics.imaging import (
            BitmapAlphaMode,
            BitmapPixelFormat,
            SoftwareBitmap,
        )
        from winrt.windows.media.ocr import OcrEngine
        from winrt.windows.security.cryptography import CryptographicBuffer
    except ImportError:
        # Keep source environments created with older releases working. The
        # packaged build uses the modular winrt wheels, which avoid shipping
        # winsdk's 36 MiB all-namespaces extension.
        try:
            from winsdk.windows.globalization import Language
            from winsdk.windows.graphics.imaging import (
                BitmapAlphaMode,
                BitmapPixelFormat,
                SoftwareBitmap,
            )
            from winsdk.windows.media.ocr import OcrEngine
            from winsdk.windows.security.cryptography import CryptographicBuffer
        except ImportError as exc:  # pragma: no cover
            raise OcrError(
                "缺少 Windows OCR 组件，请重新安装 requirements.lock"
            ) from exc
    return Language, SoftwareBitmap, BitmapPixelFormat, BitmapAlphaMode, OcrEngine, CryptographicBuffer


def available_languages() -> list[str]:
    """系统已安装的 OCR 语言标签，例如 ['zh-Hans-CN', 'en-US']。"""
    global _available_cache
    if _available_cache is None:
        try:
            _, _, _, _, OcrEngine, _ = _winrt_types()
            _available_cache = [
                lang.language_tag for lang in OcrEngine.available_recognizer_languages
            ]
        except Exception as exc:
            # 这里吞掉异常是对的（没装语言包属于正常情况），但别吞得无声无息：
            # 之前这个函数因为一处解包写错而一直返回空列表，
            # 表现是「补空格功能悄悄失灵」，查了半天才定位到。
            print(f"[ocr] 读取系统 OCR 语言列表失败：{type(exc).__name__}: {exc}")
            _available_cache = []
    return list(_available_cache)


def is_available() -> bool:
    return bool(available_languages())


def _get_engine(tag: str):
    with _lock:
        if tag not in _engine_cache:
            Language, _, _, _, OcrEngine, _ = _winrt_types()
            try:
                _engine_cache[tag] = OcrEngine.try_create_from_language(Language(tag))
            except Exception:
                _engine_cache[tag] = None
        return _engine_cache[tag]


def _to_software_bitmap(img: Image.Image):
    _, SoftwareBitmap, BitmapPixelFormat, BitmapAlphaMode, _, CryptographicBuffer = _winrt_types()
    rgba = img.convert("RGBA")
    r, g, b, a = rgba.split()
    bgra = Image.merge("RGBA", (b, g, r, a))      # WinRT 要的是 BGRA8 排列
    buf = CryptographicBuffer.create_from_byte_array(bgra.tobytes())
    create_with_alpha = getattr(
        SoftwareBitmap, "create_copy_with_alpha_from_buffer", None
    )
    if callable(create_with_alpha):
        return create_with_alpha(
            buf,
            BitmapPixelFormat.BGRA8,
            rgba.width,
            rgba.height,
            BitmapAlphaMode.PREMULTIPLIED,
        )
    return SoftwareBitmap.create_copy_from_buffer(
        buf,
        BitmapPixelFormat.BGRA8,
        rgba.width,
        rgba.height,
        BitmapAlphaMode.PREMULTIPLIED,
    )


def _deskew_fixer(angle: float, width: float, height: float):
    """把包围盒从「摆正后」的坐标系转回原图坐标系。

    Windows OCR 会先估一个倾斜角把画面摆正再识别，`OcrResult.text_angle` 就是那个角度，
    而它给出的所有包围盒都是**摆正之后**的坐标。原来的代码完全没管这个角度，
    直接当原图坐标用了——引擎一旦判定有倾斜（哪怕是它自己看花眼，
    实测一行字放在空旷画布上就能让它认出 5~6 度），整屏译文就会整体错位，
    错多少取决于画面宽度：一行字横跨 1400px、倾斜 6 度，纵向能差到 40 像素。

    摆正是绕图片中心转的，所以转回去就是绕同一个中心反向转。
    """
    if not angle:
        return None
    rad = math.radians(angle)
    cos, sin = math.cos(rad), math.sin(rad)
    cx, cy = width / 2.0, height / 2.0

    def fix(x: float, y: float, w: float, h: float) -> tuple[float, float, float, float]:
        # 只把**中心**转回去，宽高原样保留。
        # 转四个角再取外接框的话，斜矩形的外接框会比原框大一圈
        # （60px 宽的词斜 6 度，高度就凭空多出 6px），位置反而更不准。
        dx, dy = x + w / 2.0 - cx, y + h / 2.0 - cy
        nx = cx + dx * cos - dy * sin
        ny = cy + dx * sin + dy * cos
        return nx - w / 2.0, ny - h / 2.0, w, h

    return fix


def _is_cjk(ch: str) -> bool:
    o = ord(ch)
    return (
        0x3000 <= o <= 0x303F      # CJK 标点
        or 0x3040 <= o <= 0x30FF   # 假名
        or 0x3400 <= o <= 0x4DBF   # 扩展 A
        or 0x4E00 <= o <= 0x9FFF   # 基本汉字
        or 0xAC00 <= o <= 0xD7AF   # 谚文
        or 0xF900 <= o <= 0xFAFF
        or 0xFF00 <= o <= 0xFFEF   # 全角
    )


def _join_words(words: list[Word]) -> str:
    """Windows OCR 会在每个「词」之间插空格，中文因此变成「这 是 一 段」。
    这里按两侧字符是否为 CJK 决定要不要空格。"""
    out = ""
    for w in words:
        if not w.text:
            continue
        if out:
            prev, cur = out[-1], w.text[0]
            if not (_is_cjk(prev) or _is_cjk(cur)):
                out += " "
        out += w.text
    return out


def _is_han(ch: str) -> bool:
    o = ord(ch)
    return 0x3400 <= o <= 0x9FFF or 0xF900 <= o <= 0xFAFF


# 把一段文字切成「连续汉字」和「连续非汉字」两种块。
# 中英之间本来就没有空格（_join_words 是故意不加的），只按空白切分不开。
_CHUNK = re.compile(r"[㐀-鿿豈-﫿]+|[^\s㐀-鿿豈-﫿]+")


def _token_score(tok: str) -> float:
    han = sum(1 for c in tok if _is_han(c))
    if han:
        return float(han)
    alpha = sum(1 for c in tok if c.isalpha())
    if not alpha:
        return len(tok) * 0.2          # 纯符号，几乎没有信息量
    score = float(alpha)
    if sum(1 for c in tok if c.isdigit()) and alpha >= 3:
        score *= 0.4                   # 词中间夹数字：Pr0Ject、1abel，典型的形近误读
    if alpha <= 2:
        score *= 0.6                   # 碎片：中文引擎会把 river 切成 ri / ve / r
    return score


def _quality(lines: list[Line]) -> float:
    """给一份识别结果打分，用来在多个语言引擎之间挑赢家。

    不能只比「认出了多少个字」——中文引擎读英文界面时会读出**更多**字符，
    但全是坏的：o 读成 0（Project→Pr0Ject）、图标幻觉成孤立汉字（囗、匚）、
    把 river 切成 ri/ve/r。按字符数比的话垃圾反而赢，
    这正是深色界面截图被翻成乱码的真正原因。
    """
    return sum(_token_score(t) for line in lines for t in _CHUNK.findall(line.text))


def _recognize_once(img: Image.Image, tag: str) -> list[Line]:
    engine = _get_engine(tag)
    if engine is None:
        raise OcrError(f"系统未安装 {tag} 的 OCR 语言包")

    bitmap = _to_software_bitmap(img)

    async def _run():
        return await engine.recognize_async(bitmap)

    loop = asyncio.new_event_loop()
    try:
        result = loop.run_until_complete(_run())
    finally:
        loop.close()
        close = getattr(bitmap, "close", None)
        if callable(close):
            close()

    fix = _deskew_fixer(float(result.text_angle or 0.0), img.width, img.height)

    lines: list[Line] = []
    for ocr_line in result.lines:
        words = []
        for w in ocr_line.words:
            r = w.bounding_rect
            box = (r.x, r.y, r.width, r.height)
            words.append(Word(w.text, *(fix(*box) if fix else box)))
        if not words:
            continue
        x0 = min(w.x for w in words)
        y0 = min(w.y for w in words)
        x1 = max(w.x + w.w for w in words)
        y1 = max(w.y + w.h for w in words)
        lines.append(Line(_join_words(words), x0, y0, x1 - x0, y1 - y0, words))
    return lines


def recognize(
    img: Image.Image, languages: list[str] | None = None, upscale: bool = True
) -> list[Line]:
    """识别图片。languages 按顺序尝试，取识别出字符最多的那一份结果。"""
    langs = [t for t in (languages or []) if t] or available_languages()[:1]
    if not langs:
        raise OcrError(
            "系统没有可用的 OCR 语言包。\n"
            "请到「设置 → 时间和语言 → 语言和区域 → 语言选项 → 可选功能」中安装「光学字符识别」。"
        )

    # 小图放大后识别率明显更高（Windows OCR 对 <20px 的字很吃力）
    scale = 1.0
    work = img
    if upscale:
        longest = max(img.width, img.height)
        if longest < 1600:
            scale = min(3.0, max(1.0, 1400 / max(1, longest)))
        if scale > 1.05:
            work = img.resize(
                (int(img.width * scale), int(img.height * scale)), Image.LANCZOS
            )
        else:
            scale = 1.0

    # 选引擎策略：配了几种语言就**每种都真跑一遍**，按 _quality 挑赢家。
    #
    # 之前是「先跑首选，再猜要不要换一个」，猜的依据是首轮结果里汉字占比。
    # 这个猜法在深色界面截图上会翻车：中文引擎把图标幻觉成 囗、匚 之类的汉字，
    # 汉字占比被抬上去，于是判定「内容是中文」，不再换英文引擎——
    # 最后拿着 "囗Pr0Ject folder匚" 去翻译，出来自然是乱码。
    # 系统 OCR 一次只要几十毫秒，多跑一个引擎换来判断可靠，很划算。
    best: list[Line] = []
    best_score = -1.0
    errors: list[str] = []

    pool = list(dict.fromkeys(langs))[:3]      # 最多跑 3 个，别把耗时堆上去
    for tag in pool:
        try:
            lines = _recognize_once(work, tag)
        except OcrError as exc:
            errors.append(str(exc))
            continue
        s = _quality(lines)
        if s > best_score:
            best, best_score = lines, s

    if best_score < 0:
        if errors:
            raise OcrError("；".join(dict.fromkeys(errors)))
        return []

    if scale != 1.0:
        inv = 1.0 / scale
        for line in best:
            line.x *= inv
            line.y *= inv
            line.w *= inv
            line.h *= inv
            for w in line.words:
                w.x *= inv
                w.y *= inv
                w.w *= inv
                w.h *= inv
    return best
