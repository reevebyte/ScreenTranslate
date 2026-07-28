"""强调色选择器：一排小圆点，点哪个是哪个。

以前这个颜色只能手改 config.json——等于没有。它管着框选边框、拖拽手柄、
进度条、菜单高亮，是全程序最显眼的一个设置，不该藏在文件里。

给的是一排预设而不是调色盘：这个颜色要压在**任意背景**上还看得清，
随手调出来的暗紫、浅黄都不合格。预设都是挑过饱和度和亮度的。
"""
from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, QSize, Qt, Signal
from PySide6.QtGui import QColor, QPainter, QPen
from PySide6.QtWidgets import QWidget

PRESETS = [
    ("#28C76F", "草绿"),
    ("#4C8DFF", "天蓝"),
    ("#8C7CF0", "薰衣草"),
    ("#FF7A45", "橘"),
    ("#F45B7A", "樱桃"),
    ("#F2C744", "杏黄"),
    ("#00C2C7", "青"),
    ("#E8EAEE", "白"),
]

DOT = 22          # 每个色点的外接方块
GAP = 8


class SwatchRow(QWidget):
    picked = Signal(str)

    def __init__(self, current: str, parent: QWidget | None = None):
        super().__init__(parent)
        self._current = (current or PRESETS[0][0]).upper()
        self._hover = -1
        self.setMouseTracking(True)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setFixedHeight(DOT + 6)
        self.setMinimumWidth(len(PRESETS) * (DOT + GAP))
        self.setToolTip("框选边框、拖拽手柄、进度条都用这个颜色")

    def sizeHint(self) -> QSize:
        return QSize(len(PRESETS) * (DOT + GAP), DOT + 6)

    def value(self) -> str:
        return self._current

    def set_value(self, color: str) -> None:
        self._current = (color or "").upper()
        self.update()

    def _cell(self, i: int) -> QRectF:
        return QRectF(i * (DOT + GAP), 3, DOT, DOT)

    def _at(self, x: float, y: float) -> int:
        for i in range(len(PRESETS)):
            if self._cell(i).adjusted(-GAP / 2, -3, GAP / 2, 3).contains(x, y):
                return i
        return -1

    # ------------------------------------------------------------------ 绘制
    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        for i, (hexv, _name) in enumerate(PRESETS):
            cell = self._cell(i)
            c = QColor(hexv)
            on = hexv.upper() == self._current
            r = DOT / 2 - (3.5 if on else 1.5)
            if on:
                # 选中的画一圈同色细环，中间留缝——比打个勾干净，也不挑颜色深浅
                ring = QPen(c, 1.8)
                p.setPen(ring)
                p.setBrush(Qt.BrushStyle.NoBrush)
                p.drawEllipse(cell.center(), DOT / 2 - 1.0, DOT / 2 - 1.0)
            elif i == self._hover:
                p.setPen(QPen(QColor(255, 255, 255, 60), 1.4))
                p.setBrush(Qt.BrushStyle.NoBrush)
                p.drawEllipse(cell.center(), DOT / 2 - 1.0, DOT / 2 - 1.0)
            p.setPen(Qt.PenStyle.NoPen)
            p.setBrush(c)
            p.drawEllipse(cell.center(), r, r)

    # ------------------------------------------------------------------ 交互
    def mouseMoveEvent(self, event):
        i = self._at(event.position().x(), event.position().y())
        if i != self._hover:
            self._hover = i
            self.setToolTip(PRESETS[i][1] if i >= 0 else "")
            self.update()

    def leaveEvent(self, event):
        super().leaveEvent(event)
        self._hover = -1
        self.update()

    def mousePressEvent(self, event):
        if event.button() != Qt.MouseButton.LeftButton:
            return
        i = self._at(event.position().x(), event.position().y())
        if i < 0:
            return
        self._current = PRESETS[i][0].upper()
        self.update()
        self.picked.emit(self._current)
