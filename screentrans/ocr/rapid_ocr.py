"""可选的 RapidOCR 引擎（离线，支持日/韩等系统 OCR 语言包没覆盖的语言）。

需要额外安装：pip install rapidocr-onnxruntime
没装的话程序会自动隐藏这个选项，不影响使用。
"""
from __future__ import annotations

import difflib
import math
import re
import threading

from PIL import Image

from .base import Line, OcrError, Word

_engine = None
_lock = threading.Lock()

# 连续 10 个以上字母还一个空格都没有，基本就是被粘成一坨了
_GLUED = re.compile(r"[A-Za-z]{10,}")


def _engine_confidence(value: object) -> float | None:
    """Return only a finite confidence value supplied by RapidOCR itself."""
    if value is None:
        return None
    try:
        confidence = float(value)
    except (TypeError, ValueError):
        return None
    return confidence if math.isfinite(confidence) else None


def _letters(s: str) -> str:
    return re.sub(r"[^a-z0-9]", "", s.lower())


def _needs_space_repair(text: str) -> bool:
    """RapidOCR 默认用的是中文识别模型，对纯英文行经常整行不吐空格：
    'Project or folder' → 'Projectorfolder'、
    'This release focuses on' → 'Thisreleasefocuseson'。
    粘成这样再去翻译，出来的必然是乱码（投影文件夹之类）。
    中文本来就不用空格，所以含汉字的行不算。"""
    return bool(_GLUED.search(text)) and not re.search(r"[㐀-鿿]", text)


def _repair_spaces(img: Image.Image, lines: list[Line]) -> None:
    """把粘成一坨的英文行交给系统 OCR 重读——它对拉丁文的分词是准的。

    整张图只重读一次，再按「去掉空格和标点后的字母序列」跟 RapidOCR 的结果对上号，
    这样比逐行裁剪快得多。对不上就原样保留，不拿一个没把握的读法去换掉原结果。
    """
    if not any(_needs_space_repair(l.text) for l in lines):
        return
    try:
        from . import windows_ocr

        tag = next((t for t in windows_ocr.available_languages()
                    if not t.lower().startswith(("zh", "ja", "ko"))), None)
        if tag is None:
            return
        reference = windows_ocr.recognize(img, [tag])
    except Exception:
        return      # 系统 OCR 用不了就算了，修不了总比崩了强

    table: dict[str, str] = {}
    for r in reference:
        if " " in r.text.strip():
            table.setdefault(_letters(r.text), r.text.strip())
    if not table:
        return

    for line in lines:
        if not _needs_space_repair(line.text):
            continue
        key = _letters(line.text)
        fixed = table.get(key) or next(
            (v for k, v in table.items()
             if difflib.SequenceMatcher(None, k, key).ratio() >= 0.88),
            None,
        )
        if fixed:
            line.text = fixed
            for w in line.words:
                w.text = fixed


def is_installed() -> bool:
    """只看装没装，不真的导入（导入代价大，而且顺序敏感，见 _get_engine）。"""
    import importlib.util

    try:
        return importlib.util.find_spec("rapidocr_onnxruntime") is not None
    except Exception:
        return False


def is_available() -> bool:
    return is_installed()


def _get_engine():
    global _engine
    with _lock:
        if _engine is None:
            if not is_installed():
                raise OcrError("未安装 RapidOCR，请运行：pip install rapidocr-onnxruntime")
            try:
                from rapidocr_onnxruntime import RapidOCR
            except Exception as exc:
                # 装了却导入失败，最常见的原因是 onnxruntime 在 PySide6 之后加载。
                # 正常启动路径已经做了预载，会走到这里说明是运行中途才切到 RapidOCR 的。
                raise OcrError(
                    "RapidOCR 加载失败，请重启本程序后再试。\n"
                    f"（切换 OCR 引擎需要重启才能生效。原始错误：{type(exc).__name__}: "
                    f"{str(exc).splitlines()[0][:120]}）"
                ) from exc
            _engine = RapidOCR()
        return _engine


def _image_array(img: Image.Image):
    """Import NumPy only when the optional RapidOCR engine is actually used."""
    try:
        import numpy as np
    except ImportError as exc:
        raise OcrError(
            "RapidOCR 缺少 NumPy 依赖，请重新安装 requirements-rapidocr.lock"
        ) from exc
    return np.array(img.convert("RGB"))


def recognize(img: Image.Image) -> list[Line]:
    engine = _get_engine()
    result, _ = engine(_image_array(img))
    lines: list[Line] = []
    for item in result or []:
        box, text = item[0], item[1]
        score = item[2] if len(item) > 2 else None
        xs = [float(p[0]) for p in box]
        ys = [float(p[1]) for p in box]
        x0, y0, x1, y1 = min(xs), min(ys), max(xs), max(ys)
        if not text.strip():
            continue
        confidence = _engine_confidence(score)
        word = Word(text, x0, y0, x1 - x0, y1 - y0, confidence=confidence)
        lines.append(
            Line(
                text,
                x0,
                y0,
                x1 - x0,
                y1 - y0,
                words=[word],
                confidence=confidence,
            )
        )
    _repair_spaces(img, lines)
    return lines
