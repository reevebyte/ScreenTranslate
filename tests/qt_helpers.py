from __future__ import annotations

import copy
import os

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QRect
from PySide6.QtGui import QColor, QImage
from PySide6.QtWidgets import QApplication


_APP = None


def application() -> QApplication:
    global _APP
    _APP = QApplication.instance() or QApplication(["screentrans-tests"])
    return _APP


class MemoryConfig:
    def __init__(self, data: dict | None = None):
        self.data = copy.deepcopy(data or {})
        self.save_count = 0

    def get(self, path: str, default=None):
        node = self.data
        for part in path.split("."):
            if not isinstance(node, dict) or part not in node:
                return default
            node = node[part]
        return node

    def set(self, path: str, value) -> None:
        node = self.data
        parts = path.split(".")
        for part in parts[:-1]:
            node = node.setdefault(part, {})
        node[parts[-1]] = value

    def save(self) -> None:
        self.save_count += 1


def solid_image(width: int, height: int, color: QColor, dpr: float = 1.0) -> QImage:
    image = QImage(
        max(1, int(round(width * dpr))),
        max(1, int(round(height * dpr))),
        QImage.Format.Format_ARGB32,
    )
    image.fill(color)
    image.setDevicePixelRatio(dpr)
    return image


def result_fixture():
    from screentrans.result import ResultWindow

    app = application()
    screen = app.primaryScreen()
    bounds = screen.geometry()
    width = min(120, max(40, bounds.width() // 3))
    height = min(80, max(30, bounds.height() // 4))
    left = bounds.left() + max(10, (bounds.width() - width) // 3)
    top = bounds.top() + max(10, (bounds.height() - height) // 3)
    home = QRect(left, top, width, height)

    dpr = screen.devicePixelRatio() or 1.0
    original = solid_image(width, height, QColor(220, 30, 30), dpr)

    cfg = MemoryConfig(
        {
            "appearance": {
                "accent": "#28C76F",
                "close_mode": "click",
                "timeout_ms": 5000,
            }
        }
    )
    window = ResultWindow(screen, home, original, cfg)
    return app, window, home
