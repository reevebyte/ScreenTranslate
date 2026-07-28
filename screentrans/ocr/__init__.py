"""OCR 引擎注册表。"""
from __future__ import annotations

from PIL import Image

from .base import Line, OcrError, Word

ENGINES = {
    "windows": "系统自带 OCR（离线，推荐）",
    "azure_vision": "Azure AI Vision OCR（官方云端）",
    "youdao_cloud": "有道云端 OCR（截图会上传）",
    "rapidocr": "RapidOCR（可选安装，多语言）",
}


def available_engines() -> dict[str, str]:
    out = {}
    from . import windows_ocr

    if windows_ocr.is_available():
        out["windows"] = ENGINES["windows"]
    out["azure_vision"] = ENGINES["azure_vision"]
    out["youdao_cloud"] = ENGINES["youdao_cloud"]
    try:
        from . import rapid_ocr

        if rapid_ocr.is_available():
            out["rapidocr"] = ENGINES["rapidocr"]
    except Exception:
        pass
    return out


def recognize(img: Image.Image, cfg) -> list[Line]:
    engine = cfg.get("ocr.engine", "windows")
    if engine == "azure_vision":
        from . import azure_vision

        return azure_vision.recognize(img, cfg.get("ocr.azure_vision", {}) or {})
    if engine == "youdao_cloud":
        from . import youdao_cloud

        return youdao_cloud.recognize(img)
    if engine == "rapidocr":
        from . import rapid_ocr

        return rapid_ocr.recognize(img)
    from . import windows_ocr

    return windows_ocr.recognize(
        img,
        languages=cfg.get("ocr.languages") or [],
        upscale=bool(cfg.get("ocr.upscale", True)),
    )


__all__ = ["Line", "Word", "OcrError", "recognize", "available_engines", "ENGINES"]
