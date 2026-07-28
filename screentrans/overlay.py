"""框选遮罩。按下快捷键后覆盖所有屏幕，拉一个框，松手立刻开始翻译。

界面上没有任何按钮：
  拖动     选区
  松开     立即翻译
  Esc/右键 取消
"""
from __future__ import annotations

from PySide6.QtCore import QPoint, QRect, Qt, Signal
from PySide6.QtGui import QColor, QCursor, QGuiApplication, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QWidget

from . import winsys
from .ui import frame

DIM = QColor(0, 0, 0, 108)
MIN_SIZE = 6
CROSSHAIR_HALF_LENGTH = 12
CROSSHAIR_WIDTH = 2
CROSSHAIR_ALPHA = 235


class SelectionOverlay(QWidget):
    # (QScreen, 物理矩形 (l,t,w,h), 选区图像)
    selected = Signal(object, object, object)
    cancelled = Signal()

    def __init__(self, screen, pixmap: QPixmap,
                 phys_rect: tuple[int, int, int, int], accent: str):
        super().__init__()
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool
        )
        # 系统十字光标和下面自己画的短十字叠在一起会出现双线、长短不一。
        # 隐藏系统光标，只保留这一套 24px 的中央准线。
        self.setCursor(Qt.CursorShape.BlankCursor)
        self.setMouseTracking(True)

        self._screen = screen
        self._pixmap = pixmap
        self._phys = phys_rect
        self._accent = QColor(accent)
        self._origin: QPoint | None = None
        self._current: QPoint | None = None
        self._done = False

        geo = screen.geometry()
        self.setGeometry(geo)
        # 十字准线的初始位置。等第一次 mouseMove 才有值的话，遮罩刚出来的一瞬间没有线
        self._current = QCursor.pos() - geo.topLeft()
        handle = self.windowHandle()
        if handle is not None:
            handle.setScreen(screen)

    # ---------------------------------------------------------------- 绘制
    def paintEvent(self, _event):
        p = QPainter(self)
        p.drawPixmap(self.rect(), self._pixmap)

        sel = self._selection_rect()
        if sel is None or sel.isEmpty():
            p.fillRect(self.rect(), DIM)
            self._paint_crosshair(p)
            return

        # 选区之外压暗，选区内保持原样，这样能看清要翻译的到底是什么
        full = self.rect()
        p.fillRect(QRect(full.left(), full.top(), full.width(), sel.top() - full.top()), DIM)
        p.fillRect(QRect(full.left(), sel.bottom() + 1, full.width(), full.bottom() - sel.bottom()), DIM)
        p.fillRect(QRect(full.left(), sel.top(), sel.left() - full.left(), sel.height()), DIM)
        p.fillRect(QRect(sel.right() + 1, sel.top(), full.right() - sel.right(), sel.height()), DIM)

        frame.draw(p, sel, self._accent)
        frame.draw_size_badge(p, sel, self._accent, full)

    def _paint_crosshair(self, p: QPainter) -> None:
        """还没开始拖的时候，跟着鼠标画一对清晰的十字线。

        原来这一步屏幕上只有一层灰，既看不出程序进没进入框选状态，
        也不好对准要框的那一行字。
        """
        if self._current is None:
            return
        bounds = self.rect()
        if not bounds.contains(self._current):
            return
        pen = QPen(
            QColor(
                self._accent.red(),
                self._accent.green(),
                self._accent.blue(),
                CROSSHAIR_ALPHA,
            ),
            CROSSHAIR_WIDTH,
        )
        p.setPen(pen)
        x, y = self._current.x(), self._current.y()
        p.drawLine(
            max(bounds.left(), x - CROSSHAIR_HALF_LENGTH),
            y,
            min(bounds.right(), x + CROSSHAIR_HALF_LENGTH),
            y,
        )
        p.drawLine(
            x,
            max(bounds.top(), y - CROSSHAIR_HALF_LENGTH),
            x,
            min(bounds.bottom(), y + CROSSHAIR_HALF_LENGTH),
        )

    def _selection_rect(self) -> QRect | None:
        if self._origin is None or self._current is None:
            return None
        return QRect(self._origin, self._current).normalized().intersected(self.rect())

    # ---------------------------------------------------------------- 交互
    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.RightButton:
            self.cancelled.emit()
            return
        if event.button() == Qt.MouseButton.LeftButton:
            self._origin = event.position().toPoint()
            self._current = self._origin
            self.update()

    def mouseMoveEvent(self, event):
        # 没按下也要记位置：十字准线要跟着鼠标走
        self._current = event.position().toPoint()
        self.update()

    def mouseReleaseEvent(self, event):
        if event.button() != Qt.MouseButton.LeftButton or self._origin is None or self._done:
            return
        self._current = event.position().toPoint()
        sel = self._selection_rect()
        self._origin = None
        if sel is None or sel.width() < MIN_SIZE or sel.height() < MIN_SIZE:
            self.cancelled.emit()
            return
        self._done = True
        phys = self._to_physical(sel)
        # 直接从冻结的截图上裁，而不是重新截屏——
        # 否则遮罩层可能还没来得及消失，会把压暗的画面一起截进去。
        pl, pt = self._phys[0], self._phys[1]
        left, top, width, height = phys
        # Crop while it is still a pixmap. Converting the full frozen 4K screen
        # to QImage first creates a needless ~32 MiB temporary for a small box.
        frozen = self._pixmap.copy(left - pl, top - pt, width, height).toImage()
        from .render import qimage_to_pil

        crop = qimage_to_pil(frozen)
        self.selected.emit(self._screen, phys, crop)

    def _to_physical(self, rect: QRect) -> tuple[int, int, int, int]:
        """窗口内的逻辑坐标 -> 虚拟桌面的物理像素坐标。"""
        dpr = self._screen.devicePixelRatio()
        pl, pt, pw, ph = self._phys
        left = pl + int(round(rect.left() * dpr))
        top = pt + int(round(rect.top() * dpr))
        width = max(1, int(round(rect.width() * dpr)))
        height = max(1, int(round(rect.height() * dpr)))
        # 夹回本屏范围，避免因为取整多截到隔壁屏幕的一列像素
        left = max(pl, min(left, pl + pw - 1))
        top = max(pt, min(top, pt + ph - 1))
        width = min(width, pl + pw - left)
        height = min(height, pt + ph - top)
        return left, top, width, height

    def keyPressEvent(self, event):
        if event.key() in (Qt.Key.Key_Escape, Qt.Key.Key_Q):
            self.cancelled.emit()


class OverlayManager:
    """管理所有屏幕上的遮罩窗口，保证一起出现、一起消失。"""

    def __init__(self, accent: str = "#4C8DFF"):
        self._accent = accent
        self._overlays: list[SelectionOverlay] = []
        self._on_selected = None
        self._on_cancelled = None

    @property
    def active(self) -> bool:
        return bool(self._overlays)

    def set_accent(self, accent: str) -> None:
        """换强调色。遮罩是每次框选现建的，下一次就会用新颜色。"""
        self._accent = accent

    def start(self, on_selected, on_cancelled=None) -> None:
        from . import capture
        from .render import pil_to_qimage

        if self._overlays:
            return
        self._on_selected = on_selected
        self._on_cancelled = on_cancelled

        cursor_pos = QCursor.pos()
        focus_target = None
        for screen in QGuiApplication.screens():
            phys = winsys.screen_physical_rect(screen)
            try:
                shot = capture.grab(*phys)
            except Exception as exc:
                print(f"[overlay] 截屏失败 {screen.name()}: {exc}")
                continue
            pixmap = QPixmap.fromImage(pil_to_qimage(shot))
            pixmap.setDevicePixelRatio(screen.devicePixelRatio())

            ov = SelectionOverlay(screen, pixmap, phys, self._accent)
            ov.selected.connect(self._handle_selected)
            ov.cancelled.connect(self._handle_cancelled)
            self._overlays.append(ov)
            if screen.geometry().contains(cursor_pos):
                focus_target = ov

        for ov in self._overlays:
            ov.show()
            ov.raise_()

        target = focus_target or (self._overlays[0] if self._overlays else None)
        if target is not None:
            target.activateWindow()
            target.setFocus(Qt.FocusReason.OtherFocusReason)
            target.grabKeyboard()

    def close(self) -> None:
        for ov in self._overlays:
            ov.releaseKeyboard()
            ov.hide()
            ov.deleteLater()
        self._overlays.clear()

    def _handle_selected(self, screen, phys_rect, crop):
        cb = self._on_selected
        # ResultWindow has grown substantially since the first version. Build
        # and show it while the frozen screenshot is still covering the live
        # desktop, then remove the overlays immediately. This preserves the
        # original 110 ms result fade without exposing constructor time as a
        # visible blank frame.
        for ov in self._overlays:
            ov.releaseKeyboard()
            ov.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        try:
            if cb:
                cb(screen, phys_rect, crop)
        finally:
            self.close()

    def _handle_cancelled(self):
        cb = self._on_cancelled
        self.close()
        if cb:
            cb()
