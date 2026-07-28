"""屏幕截取。坐标一律是物理像素的虚拟桌面坐标（多屏拼在一起的那个大坐标系）。"""
from __future__ import annotations

import threading

import mss
from PIL import Image

_local = threading.local()


def _sct():
    # mss 的实例不能跨线程复用
    if getattr(_local, "sct", None) is None:
        _local.sct = mss.mss()
    return _local.sct


def grab(left: int, top: int, width: int, height: int) -> Image.Image:
    width = max(1, int(width))
    height = max(1, int(height))
    raw = _sct().grab(
        {"left": int(left), "top": int(top), "width": width, "height": height}
    )
    return Image.frombytes("RGB", raw.size, raw.bgra, "raw", "BGRX")


def close() -> None:
    """Release the cached MSS device contexts owned by the current thread."""
    sct = getattr(_local, "sct", None)
    if sct is None:
        return
    try:
        sct.close()
    finally:
        del _local.sct
