"""译文覆盖窗口。

窗口本身就是一张图，没有标题栏、没有边框装饰。
「缩小」和「关闭」做成一根独立的小控制条，贴在选区**右下角外侧**，
所以它一个像素都不会挡住译文。

    Esc / ✕            关闭
    M / Ctrl+M / —     缩到托盘（也可以用全局快捷键来回切）
    拖动控制条 / 方向键  移动（画面内部不响应拖动，那一层留给划选文字）
    点画面空白处         取消当前划选
    拖动边框 / 四个角     改「框住屏幕上的哪一块」，松手重新截图重新翻译。
                        画面**不会**被拉伸变形；新框进来的部分先标一层色。
                        按住 Shift 才锁长宽比。
                        八个可拖的点鼠标移进来就显示出来，移开自动收掉
    方向键              微调位置（配合 Shift 步子更大）
    双击空白处 / Home    恢复原始大小和位置
    按住空格 / 右键      临时看回原文
    Ctrl+C / Ctrl+A     复制
"""
from __future__ import annotations

from PySide6.QtCore import (
    QEasingCurve,
    QPoint,
    QPointF,
    QPropertyAnimation,
    QRect,
    QRectF,
    QSize,
    Qt,
    QTimer,
    Signal,
)
from PySide6.QtGui import QColor, QCursor, QFont, QImage, QPainter, QPen
from PySide6.QtWidgets import (
    QApplication,
    QHBoxLayout,
    QLabel,
    QToolButton,
    QWidget,
)

from . import winsys
from .ui import frame, glyphs

BTN = 20            # 按钮边长。挪到框外之后不用再跟内容抢地方了，可以做小一点
BAR_PAD = 4
BAR_GAP = 5         # 控制条与选区之间留的缝
GRIP_W = 10         # 控制条左端那几个点，提示「这里可以拖」

# 边缘拖拽热区宽度（逻辑像素）。和手柄方块一样宽，看到多大就能拖多大。
EDGE = frame.HANDLE
# 缩放时的下限。但绝不能因此把窗口撑得比原选区还大——框选允许小到 6×6，
# 真按 72×32 兜底的话，选一个短单词得到的覆盖层会比原文大一圈、直接错位。
MIN_W, MIN_H = 72, 32

# 拖到哪个方向该显示什么光标
_CURSORS = {
    "l": Qt.CursorShape.SizeHorCursor, "r": Qt.CursorShape.SizeHorCursor,
    "t": Qt.CursorShape.SizeVerCursor, "b": Qt.CursorShape.SizeVerCursor,
    "tl": Qt.CursorShape.SizeFDiagCursor, "br": Qt.CursorShape.SizeFDiagCursor,
    "tr": Qt.CursorShape.SizeBDiagCursor, "bl": Qt.CursorShape.SizeBDiagCursor,
}


class _BarButton(QToolButton):
    """控制条上的一个按钮。图形是画出来的，不是打出来的字。

    用字符「—」「✕」的话，同一串字在不同字体下粗细、长短、基线全不一样，
    两个按钮并排就明显不匀。图形统一走 `ui.glyphs`，跟设置界面里的是同一套。
    """

    def __init__(self, kind: str, glyph: str, tip: str, parent: QWidget):
        super().__init__(parent)
        self._kind = kind
        self.setText(glyph)          # 只用来标识和读屏，实际不画出来
        self.setToolTip(tip)
        self.setFixedSize(BTN, BTN)
        self.setCursor(Qt.CursorShape.ArrowCursor)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_Hover, True)   # 悬停时才会重绘

    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        hot = self.isEnabled() and self.underMouse()

        if hot or self.isDown():
            if self._kind == "close":
                fill = QColor(232, 82, 72, 255 if hot else 205)
            else:
                fill = QColor(255, 255, 255, 74 if self.isDown() else 46)
            p.setPen(Qt.PenStyle.NoPen)
            p.setBrush(fill)
            p.drawRoundedRect(QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5), 5, 5)

        alpha = 245 if hot else (200 if self.isEnabled() else 82)
        color = QColor(255, 255, 255, alpha)
        glyphs.paint(p, self._kind, QRectF(self.rect()).adjusted(3, 3, -3, -3),
                     color, weight=2.1)


class _ControlBar(QWidget):
    """贴在选区右下角外面的一根小条：拖动手柄 + 缩小 + 关闭。

    做成独立的顶层窗口（parent 指向译文窗口，带 Tool 标志），
    这样它才能画在选区**之外**——译文窗口的大小就是选区的大小，
    内部一个多余的像素都没有。挂着 parent 是为了跟着一起销毁、一起压在上面。
    """

    retryClicked = Signal()
    editClicked = Signal()
    minimizeClicked = Signal()
    closeClicked = Signal()
    dragged = Signal(QPoint)     # 拖动位移（全局坐标），交给译文窗口去挪自己

    def __init__(self, parent: QWidget):
        super().__init__(parent)
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool
            | Qt.WindowType.WindowDoesNotAcceptFocus
        )
        # 不抢焦点：焦点得留在译文窗口上，否则点完按钮 Esc 就失灵了
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating, True)
        self.setToolTip("拖动这里可以移动窗口")

        lay = QHBoxLayout(self)
        lay.setContentsMargins(BAR_PAD + GRIP_W, BAR_PAD, BAR_PAD, BAR_PAD)
        lay.setSpacing(2)

        self._edit = _BarButton("edit", "编辑", "校对识别文字（E）", self)
        self._edit.setEnabled(False)
        self._edit.clicked.connect(self.editClicked)
        lay.addWidget(self._edit)

        self._retry = _BarButton("retry", "↻", "用当前接口重新翻译（R）", self)
        self._retry.clicked.connect(self.retryClicked)
        lay.addWidget(self._retry)

        self._min = _BarButton("minus", "—", "缩到托盘（M）", self)
        self._min.clicked.connect(self.minimizeClicked)
        lay.addWidget(self._min)

        self._close = _BarButton("close", "✕", "关闭（Esc）", self)
        self._close.setObjectName("CloseBtn")
        self._close.clicked.connect(self.closeClicked)
        lay.addWidget(self._close)

        self.setFixedSize(self.sizeHint())
        self._press: QPoint | None = None

    def set_edit_enabled(self, enabled: bool) -> None:
        self._edit.setEnabled(enabled)

    def showEvent(self, event):
        super().showEvent(event)
        # 光靠 Qt.WindowDoesNotAcceptFocus 这一位落不下去，显示之后再补一次。
        # 每次显示都补是因为窗口句柄可能被重建（换屏幕、改窗口标志都会）。
        winsys.deny_activation(int(self.winId()))

    # ------------------------------------------------------------------ 外观
    def paintEvent(self, _event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        r = QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5)
        # 圆到几乎是个胶囊，跟方棱的选区边框拉开对比，也显得软一点
        radius = r.height() / 2
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor(0, 0, 0, 46))
        p.drawRoundedRect(r.adjusted(-0.6, 0.2, 0.6, 1.4), radius, radius)   # 底下垫一层影
        p.setPen(QPen(QColor(255, 255, 255, 26), 1))
        p.setBrush(QColor(28, 30, 36, 242))
        p.drawRoundedRect(r, radius, radius)

        # 左端两列小点：告诉人这根条本身可以拖
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor(255, 255, 255, 80))
        cx = BAR_PAD + GRIP_W / 2
        cy = self.height() / 2
        for dy in (-3.4, 0.0, 3.4):
            for dx in (-1.7, 1.7):
                p.drawEllipse(QRectF(cx + dx - 0.8, cy + dy - 0.8, 1.6, 1.6))

    # ------------------------------------------------------ 拖动 = 移动窗口
    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self._press = event.globalPosition().toPoint()
            self.setCursor(Qt.CursorShape.SizeAllCursor)

    def mouseMoveEvent(self, event):
        if self._press is not None:
            now = event.globalPosition().toPoint()
            self.dragged.emit(now - self._press)
            self._press = now

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton and self._press is not None:
            self._press = None
            self.unsetCursor()


class _SelectableText(QLabel):
    """铺在译文上的透明文字层：看不见，但能用鼠标划选、能复制。

    真正显示出来的字是画在底图上的（那样才能贴合原版式、用原文的颜色），
    这一层只负责提供「可选中」的能力，所以文字本身设成全透明，
    只让选区高亮透出来盖在底图的字上面。
    """

    def __init__(self, text: str, parent: QWidget):
        super().__init__(text, parent)
        self._edge_resize = False
        self.setWordWrap(True)
        self.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.NoContextMenu)
        # 不要焦点：Esc / 空格 / Ctrl+C 统一交给窗口处理
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.setCursor(Qt.CursorShape.IBeamCursor)
        self.setMouseTracking(True)
        # QLabel 默认带 indent/margin，会让选区高亮相对底图的字整体偏移，清零对齐
        self.setMargin(0)
        self.setIndent(0)
        self.setStyleSheet(
            "background: transparent;"
            "color: rgba(0,0,0,0);"
            "selection-background-color: rgba(76,141,255,110);"
            "selection-color: rgba(0,0,0,0);"
        )

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.RightButton:
            # 右键是「临时看原文」，交给窗口处理，别被文字层吃掉
            self.parent().set_peek(True)
            return
        if (event.button() == Qt.MouseButton.LeftButton
                and self.parent()._begin_resize_at(event.globalPosition().toPoint())):
            self._edge_resize = True
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event):
        parent = self.parent()
        if self._edge_resize:
            parent._drag_resize_to(event.globalPosition().toPoint(), event.modifiers())
            event.accept()
            return
        zone = parent._zone_at(parent.mapFromGlobal(event.globalPosition().toPoint()))
        self.setCursor(_CURSORS.get(zone, Qt.CursorShape.IBeamCursor))
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.MouseButton.RightButton:
            self.parent().set_peek(False)
            return
        if event.button() == Qt.MouseButton.LeftButton and self._edge_resize:
            self._edge_resize = False
            self.parent()._finish_resize()
            self.setCursor(Qt.CursorShape.IBeamCursor)
            event.accept()
            return
        super().mouseReleaseEvent(event)


class ResultWindow(QWidget):
    closed = Signal()
    minimized = Signal()
    retry = Signal()
    editRequested = Signal()
    regionChanged = Signal()     # 拖过边框，框住的屏幕范围变了，得重新截一次重新翻

    def __init__(self, screen, logical_rect: QRect, original: QImage, cfg):
        super().__init__()
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool
        )
        self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose, True)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setCursor(Qt.CursorShape.ArrowCursor)

        self._cfg = cfg
        self._dpr = screen.devicePixelRatio() or 1.0
        self._accent = QColor(cfg.get("appearance.accent", "#4C8DFF"))
        self._close_mode = cfg.get("appearance.close_mode", "click")

        dpr = self._dpr
        self._original = QImage(original)
        self._original.setDevicePixelRatio(dpr)
        self._translated: QImage | None = None
        self._peek = False
        self._busy = True
        self._editing = False
        self._minimized = False
        self._error: str | None = None
        self._phase = 0.0
        self.translated_text = ""

        # 下面这几个必须赶在 setGeometry 之前建好：setGeometry 会同步触发
        # moveEvent / resizeEvent，而那两个回调要用到它们。
        self._bar: _ControlBar | None = None
        self._labels: list[_SelectableText] = []
        self._specs: list = []          # render 给的排版信息，缩放时要按它重算
        # 双击「恢复原状」用的原始位置，以及缩放时的基准尺寸
        self._home = QRect(logical_rect)
        self._base = QSize(max(1, logical_rect.width()), max(1, logical_rect.height()))
        self._ratio = self._base.width() / self._base.height()
        # 画面内容对应屏幕上的哪个位置（左上角，逻辑坐标）。
        # 图是**钉在屏幕上**的，不是钉在窗口上：拖边框放大时，原来那块内容
        # 不该跟着一起被拉大，它对应的还是屏幕上原来那几个像素。
        self._img_pos = QPoint(logical_rect.topLeft())
        # 最近一次真正截到的屏幕区域；窗口整体挪动只是在摆放译文，不改它。
        # Home 是否要重新截图应比较这个，而不是只比较窗口宽高。
        self._captured = QRect(logical_rect)
        self._recapturing = False

        self._min_w = min(MIN_W, self._base.width())
        self._min_h = min(MIN_H, self._base.height())

        self.setGeometry(logical_rect)
        self.setMinimumSize(self._min_w, self._min_h)
        handle = self.windowHandle()
        if handle is not None:
            handle.setScreen(screen)

        self._spin = QTimer(self)
        self._spin.timeout.connect(self._tick)
        self._spin.start(16)

        # 译文还没出来之前不响应任何自动关闭（timeout / 鼠标移开那两种模式）：
        # 等接口返回要一两秒，这期间窗口就没了体验很糟。
        self._grace = True

        self._leave_timer: QTimer | None = None
        self._close_timer: QTimer | None = None
        self._fade: QPropertyAnimation | None = None
        self._resize_zone = ""                   # 缩放：正在拖的是哪条边
        self._resize_from: QPoint | None = None
        self._resize_geo = QRect()
        self._cursor = Qt.CursorShape.ArrowCursor

        # 八个拖拽手柄的不透明度。鼠标进来淡入、离开淡出——
        # 平时不该有八个方块压在译文上，但真要拖的时候得看得见能拖哪儿。
        self._handles = 0.0
        self._handles_to = 0.0
        self._handle_anim = QTimer(self)
        self._handle_anim.setInterval(16)
        self._handle_anim.timeout.connect(self._step_handles)

        self._bar = _ControlBar(self)
        self._bar.closeClicked.connect(self.close)
        self._bar.minimizeClicked.connect(self.minimize)
        self._bar.retryClicked.connect(self.request_retry)
        self._bar.editClicked.connect(self.request_edit)
        self._bar.dragged.connect(self._nudge)
        self.setMouseTracking(True)

    # ------------------------------------------------------------ 重新框范围
    def begin_recapture(self) -> None:
        """要重新截屏了，先把自己变透明——不然截到的是自己画的译文。

        用透明而不是 hide()：hide 会丢焦点、会打乱 z 序，回来时窗口可能被别人压住；
        透明只是不参与合成，截屏拿不到它，但窗口本身还在原位。
        """
        self._recapturing = True
        self._stop_auto_close()
        if self._fade is not None:
            self._fade.stop()
        if self._bar is not None:
            self._bar.hide()
        self.setWindowOpacity(0.0)

    def end_recapture(self, original: QImage | None, region: QRect | None = None,
                      screen=None) -> None:
        """截完了。换上新画面，清掉旧译文，回到「翻译中」的状态。"""
        self._recapturing = False
        self.setWindowOpacity(1.0)
        if self._bar is not None:
            self._sync_bar()
            self._bar.show()
            self._bar.raise_()
        if original is None:
            return
        if screen is not None:
            self._dpr = screen.devicePixelRatio() or 1.0
        self._original = QImage(original)
        self._original.setDevicePixelRatio(self._dpr)
        self._captured = QRect(region if region is not None else self.geometry())
        self._img_pos = self._captured.topLeft()
        self._translated = None
        self._peek = False
        self._error = None
        self._busy = True
        self._grace = True
        for lbl in self._labels:
            lbl.deleteLater()
        self._labels.clear()
        self._specs = []
        self._spin.start(16)
        if self._bar is not None:
            self._bar.set_edit_enabled(False)
        self.update()

    def request_retry(self) -> None:
        """用当前设置重新翻译一遍。文字不重认，只重翻。"""
        if self._busy:
            return
        self._error = None
        self._busy = True
        self._grace = True
        self._stop_auto_close()
        for lbl in self._labels:
            lbl.deleteLater()
        self._labels.clear()
        self._specs = []
        self._spin.start(16)
        if self._bar is not None:
            self._bar.set_edit_enabled(False)
        self.update()
        self.retry.emit()

    def request_edit(self) -> None:
        """打开 OCR 校对入口；识别或翻译尚未完成时没有可编辑内容。"""
        if self._editing or self._busy or self._translated is None:
            return
        self._editing = True
        self._stop_auto_close()
        if self._bar is not None:
            self._bar.set_edit_enabled(False)
        self.editRequested.emit()

    def editing_finished(self) -> None:
        """校对窗口关闭后恢复原先的自动关闭策略。"""
        self._editing = False
        if self._bar is not None:
            self._bar.set_edit_enabled(not self._busy and self._translated is not None)
        if not self._busy and self._error is None:
            self._arm_auto_close()

    # ---------------------------------------------------------------- 控制条
    def _sync_bar(self) -> None:
        """把控制条摆到选区右下角外侧；屏幕下方放不下就翻到框内贴着底边。"""
        if self._bar is None:
            return
        size = self._bar.size()
        geo = self.geometry()
        # 用当前所在的屏幕，不是刚框选时那块——窗口是可以拖到别的显示器上的
        active_screen = self.screen() or QApplication.primaryScreen()
        avail = active_screen.availableGeometry() if active_screen is not None else self.geometry()

        x = geo.right() + 1 - size.width()
        y = geo.bottom() + 1 + BAR_GAP
        if y + size.height() > avail.bottom():
            y = geo.bottom() + 1 - size.height() - BAR_GAP
        x = max(avail.left(), min(x, avail.right() + 1 - size.width()))
        y = max(avail.top(), y)
        self._bar.move(x, y)

    def minimize(self) -> None:
        """缩到托盘：窗口只是藏起来，内容和位置都还在，随时能叫回来。"""
        if self._minimized or not self.isVisible():
            return
        self._minimized = True
        self._stop_auto_close()
        self.hide()
        self.minimized.emit()

    def restore_from_tray(self) -> None:
        """从托盘恢复，并从完整时长重新启动当前自动关闭策略。"""
        if not self._minimized:
            return
        self._minimized = False
        self.show()
        self.raise_()
        self.activateWindow()
        if not self._busy and self._error is None and not self._editing:
            self._arm_auto_close()

    def moveEvent(self, event):
        super().moveEvent(event)
        self._sync_bar()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._relayout_labels()
        self._sync_bar()

    def showEvent(self, event):
        super().showEvent(event)
        self._sync_bar()
        self._bar.show()
        self._bar.raise_()

    def hideEvent(self, event):
        super().hideEvent(event)
        if self._bar is not None:
            self._bar.hide()

    # ---------------------------------------------------------------- 状态
    def original_image(self) -> QImage:
        """Return an implicitly shared copy for an occasional retry render."""
        return QImage(self._original)

    def show_at(self) -> None:
        self.setWindowOpacity(0.0)
        self.show()
        self.raise_()
        self.activateWindow()
        self._fade = QPropertyAnimation(self, b"windowOpacity", self)
        self._fade.setDuration(110)
        self._fade.setStartValue(0.0)
        self._fade.setEndValue(1.0)
        self._fade.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._fade.start()

    def set_result(self, image: QImage, text: str, layouts=None) -> None:
        self._translated = QImage(image)
        self._translated.setDevicePixelRatio(self._dpr)
        self.translated_text = text
        self._busy = False
        self._error = None
        self._spin.stop()
        if self._bar is not None:
            self._bar.set_edit_enabled(not self._editing)
        self._build_text_layer(layouts or [])
        self.update()
        self._grace = False
        if not self._editing:
            self._arm_auto_close()

    def _build_text_layer(self, layouts) -> None:
        """按 render 给出的排版信息，在译文上方铺一层透明可选文字。"""
        for lbl in self._labels:
            lbl.deleteLater()
        self._labels.clear()
        self._specs = [lay for lay in layouts if lay.text.strip()]

        for lay in self._specs:
            lbl = _SelectableText(lay.text, self)
            lbl.setAlignment(Qt.AlignmentFlag(lay.align) | Qt.AlignmentFlag.AlignTop)
            lbl.show()
            self._labels.append(lbl)
        self._relayout_labels()

        # 文字层是后建的，会盖在按钮上面，得把控制条重新提到最前
        if self._bar is not None:
            self._bar.raise_()

    def _relayout_labels(self) -> None:
        """把透明文字层摆到和底图上的字重合的位置。

        底图不再缩放了（拖边框是「多框一点」，不是「把图拉大」），
        所以这里也只做平移：跟着底图一起相对窗口偏移就行。
        """
        if not self._labels:
            return
        dpr = self._dpr
        off = self._img_pos - self.pos()
        for lbl, lay in zip(self._labels, self._specs):
            font = QFont(lay.font)
            font.setPixelSize(max(1, int(round(lay.font.pixelSize() / dpr))))
            lbl.setFont(font)
            lbl.setGeometry(
                off.x() + int(round(lay.rect.left() / dpr)),
                off.y() + int(round(lay.rect.top() / dpr)),
                int(round(lay.rect.width() / dpr)),
                int(round(lay.rect.height() / dpr)),
            )

    def selected_text(self) -> str:
        parts = [l.selectedText() for l in self._labels if l.selectedText()]
        return "\n".join(parts)

    def _set_labels_visible(self, visible: bool) -> None:
        for lbl in self._labels:
            lbl.setVisible(visible)

    def set_error(self, message: str) -> None:
        # 出错就不自动关了。原来是 4 秒后关掉，但报错往往是「密钥没填」「被代理挡了」
        # 这种需要看清楚的信息，没读完就消失等于白报；而且窗口本身的设计就是
        # 「只有 ✕ 和 Esc 能关」，错误框偷偷自己跑掉也不一致。
        self._busy = False
        self._error = message
        self._spin.stop()
        self._grace = False
        if self._bar is not None:
            self._bar.set_edit_enabled(self._translated is not None and not self._editing)
        self.update()

    def _arm_auto_close(self) -> None:
        self._stop_auto_close()
        if self._minimized or self._editing or self._busy or self._error is not None:
            return
        if self._close_mode == "timeout":
            self._close_timer = QTimer(self)
            self._close_timer.setSingleShot(True)
            self._close_timer.timeout.connect(self.close)
            self._close_timer.start(int(self._cfg.get("appearance.timeout_ms", 5000)))
        elif self._close_mode == "leave":
            self._leave_timer = QTimer(self)
            self._leave_timer.timeout.connect(self._check_leave)
            self._leave_timer.start(150)

    def _stop_auto_close(self) -> None:
        if self._close_timer is not None:
            self._close_timer.stop()
            self._close_timer.deleteLater()
            self._close_timer = None
        if self._leave_timer is not None:
            self._leave_timer.stop()
            self._leave_timer.deleteLater()
            self._leave_timer = None

    def _check_leave(self) -> None:
        if self._grace or self._resize_zone:
            return
        pos = QCursor.pos()
        # 鼠标挪到控制条上不算「移开了」，否则想点 ✕ 都点不到
        if self.geometry().contains(pos):
            return
        if self._bar is not None and self._bar.geometry().contains(pos):
            return
        self.close()

    def _tick(self) -> None:
        self._phase = (self._phase + 0.022) % 1.0
        self.update()

    # ------------------------------------------------------------ 手柄淡入淡出
    def _aim_handles(self, target: float) -> None:
        self._handles_to = target
        if abs(self._handles - target) > 0.01:
            self._handle_anim.start()

    def _step_handles(self) -> None:
        gap = self._handles_to - self._handles
        if abs(gap) <= 0.06:
            self._handles = self._handles_to
            self._handle_anim.stop()
        else:
            self._handles += gap * 0.28
        self.update()

    def enterEvent(self, event):
        super().enterEvent(event)
        self._aim_handles(1.0)

    def leaveEvent(self, event):
        super().leaveEvent(event)
        if not self._resize_zone:      # 拖到框外面去了也得一直显示着
            self._aim_handles(0.0)

    # ---------------------------------------------------------------- 绘制
    def paintEvent(self, _event):
        p = QPainter(self)
        p.setCompositionMode(QPainter.CompositionMode.CompositionMode_Source)
        p.fillRect(self.rect(), Qt.GlobalColor.transparent)
        p.setCompositionMode(QPainter.CompositionMode.CompositionMode_SourceOver)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        rect = self.rect()

        show_original = self._peek or self._translated is None
        pix = self._original if show_original else self._translated
        # 按原尺寸画在它本来对应的屏幕位置上，不缩放。
        # 以前是 drawPixmap(rect, pix) 铺满窗口——拖一下边框，整张图连字带背景
        # 一起被拉变形，那不是「多框一点」，是「把图拉长」。
        off = self._img_pos - self.pos()
        p.drawImage(off, pix)

        if self._busy:
            p.fillRect(rect, QColor(0, 0, 0, 46))
            self._paint_progress(p, rect)
        elif self._error:
            self._paint_error(p, rect)

        # 边框和八个手柄跟框选遮罩用的是同一套画法，两个界面才像一个东西。
        # 手柄平时收着，鼠标进来才淡入——读译文的时候不该有八个方块杵在字上。
        frame.draw(p, rect, self._accent, self._handles)
        if self._resize_zone:
            frame.draw_size_badge(p, rect, self._accent, rect)

    def _paint_progress(self, p: QPainter, rect: QRect) -> None:
        """顶部一条来回滑动的细线，是唯一的等待提示。"""
        bar_w = max(28, rect.width() // 4)
        travel = max(1, rect.width() - bar_w)
        t = self._phase * 2
        pos = t if t <= 1 else 2 - t
        eased = pos * pos * (3 - 2 * pos)
        p.fillRect(QRect(int(eased * travel), 0, bar_w, 2), self._accent)

    def _paint_error(self, p: QPainter, rect: QRect) -> None:
        p.fillRect(rect, QColor(24, 24, 28, 232))
        p.setPen(QColor(240, 240, 245))
        font = p.font()
        font.setPixelSize(max(11, min(14, rect.height() // 6)))
        p.setFont(font)
        p.drawText(
            rect.adjusted(10, 8, -10, -8),
            int(Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignTop | Qt.TextFlag.TextWordWrap),
            self._error or "",
        )

    # ---------------------------------------------------------------- 交互
    def set_peek(self, on: bool) -> None:
        """临时显示原图。文字层一起藏起来，免得选区高亮浮在原文上。"""
        if self._peek == on:
            return
        self._peek = on
        self._set_labels_visible(not on)
        self.update()

    def reset_geometry(self) -> None:
        """回到刚框选出来的位置和大小——挪歪了想重新对上原文时用。"""
        if self._recapturing:
            return
        if self.geometry() == self._home and self._captured == self._home:
            return
        needs_recapture = self._captured != self._home
        self._img_pos = QPoint(self._home.topLeft())
        self.setGeometry(self._home)
        self._relayout_labels()
        if needs_recapture:
            # 最近截到的不是 home 那块屏幕区域，位置或尺寸任一变过都得重截。
            self.regionChanged.emit()

    def _zone_at(self, pos: QPoint) -> str:
        """判断鼠标落在哪条边的拖拽热区上。返回 "" 表示在中间。"""
        z = ""
        if pos.y() < EDGE:
            z += "t"
        elif pos.y() >= self.height() - EDGE:
            z += "b"
        if pos.x() < EDGE:
            z += "l"
        elif pos.x() >= self.width() - EDGE:
            z += "r"
        return z

    def _nudge(self, delta: QPoint) -> None:
        """整体挪窗口。画面跟着一起走——挪位置不改变框住的是哪块屏幕内容，
        只有拖边框才改。"""
        self._img_pos += delta
        self.move(self.pos() + delta)
        self._relayout_labels()

    def clear_selection(self) -> None:
        for lbl in self._labels:
            lbl.setSelection(0, 0)

    def _begin_resize_at(self, global_pos: QPoint) -> bool:
        if self._recapturing:
            return False
        zone = self._zone_at(self.mapFromGlobal(global_pos))
        if not zone:
            return False
        self._resize_zone = zone
        self._resize_from = QPoint(global_pos)
        self._resize_geo = QRect(self.geometry())
        return True

    def _drag_resize_to(self, global_pos: QPoint, modifiers) -> None:
        if not self._resize_zone or self._resize_from is None:
            return
        lock = bool(modifiers & Qt.KeyboardModifier.ShiftModifier)
        self.setGeometry(self._resized_rect(global_pos - self._resize_from, lock))

    def _finish_resize(self) -> None:
        resized = bool(self._resize_zone) and self.geometry() != self._resize_geo
        self._resize_zone = ""
        self._resize_from = None
        self._cursor = Qt.CursorShape.ArrowCursor
        self.setCursor(self._cursor)
        if not self.rect().contains(self.mapFromGlobal(QCursor.pos())):
            self._aim_handles(0.0)
        if resized:
            # 松手才重截重翻。拖的过程中每动一像素就翻一次，接口会被打爆。
            self.regionChanged.emit()

    def mousePressEvent(self, event):
        # 画面内部**不**响应拖动移动窗口：这一层要留给划选文字。
        # 想挪窗口就拖右下角那根控制条（上面那几个点就是拖拽手柄），或者用方向键。
        # 点空白处 = 取消当前的划选，和别处的文本框一个习惯。
        # 关闭一律走 ✕ 或 Esc，避免误触把结果点没了。
        if event.button() == Qt.MouseButton.LeftButton:
            if not self._begin_resize_at(event.globalPosition().toPoint()):
                self.clear_selection()
        elif event.button() == Qt.MouseButton.RightButton:
            self.set_peek(True)

    def mouseMoveEvent(self, event):
        if self._resize_zone and self._resize_from is not None:
            self._drag_resize_to(event.globalPosition().toPoint(), event.modifiers())
            return
        shape = _CURSORS.get(self._zone_at(event.position().toPoint()), Qt.CursorShape.ArrowCursor)
        if shape != self._cursor:
            self._cursor = shape
            self.setCursor(shape)

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self._finish_resize()
        elif event.button() == Qt.MouseButton.RightButton:
            self.set_peek(False)

    def mouseDoubleClickEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.reset_geometry()

    def _resized_rect(self, delta: QPoint, lock_ratio: bool) -> QRect:
        """把一次拖拽换算成新的窗口矩形。

        四条边、四个角**默认都是自由拉伸**，拖哪儿只改哪儿。
        因为拖动的含义已经变了：现在拖的是「框住屏幕上的哪一块」，不是
        「把这张图拉多大」。想多框进右边那半行字，结果高度被按比例带着一起变，
        那就是在跟人作对。要锁长宽比就按住 Shift。
        """
        g = self._resize_geo
        z = self._resize_zone
        left = g.left() + (delta.x() if "l" in z else 0)
        top = g.top() + (delta.y() if "t" in z else 0)
        right = g.right() + (delta.x() if "r" in z else 0)
        bottom = g.bottom() + (delta.y() if "b" in z else 0)

        w = max(self._min_w, right - left + 1)
        h = max(self._min_h, bottom - top + 1)

        if lock_ratio and len(z) == 2:        # len==2 就是角（tl/tr/bl/br）
            # 把这次拖拽**投影到对角线方向**，算出一个统一的缩放系数。
            #
            # 不能写成「哪边动得多就以哪边为准」：那是个分支，鼠标斜着晃的时候
            # 主导轴会在两边来回翻，窗口就跟着一跳一跳。投影是连续的，不会跳。
            # 基准也必须用**按下那一刻**的宽高，不能用最初框选时的长宽比——
            # 先拉过边框把形状改了、再去抓角，用老比例会当场弹回去。
            bw, bh = max(1, g.width()), max(1, g.height())
            s = 1.0 + ((w - bw) * bw + (h - bh) * bh) / float(bw * bw + bh * bh)
            s = max(s, self._min_w / bw, self._min_h / bh)
            w = max(self._min_w, int(round(bw * s)))
            h = max(self._min_h, int(round(bh * s)))

        # 拖的是左边/上边时，对面那条边要钉住不动
        x = (g.right() + 1 - w) if "l" in z else g.left()
        y = (g.bottom() + 1 - h) if "t" in z else g.top()
        # 每次 OCR/渲染只对应一块屏幕的一套 DPR。拖到显示器边缘就收住，避免一个
        # 逻辑矩形横跨两种缩放比例后出现截图尺寸和文字坐标都对不上的情况。
        rect = QRect(x, y, w, h)
        active_screen = self.screen() or QApplication.primaryScreen()
        bounds = active_screen.geometry() if active_screen is not None else self.geometry()
        return rect.intersected(bounds)

    def keyPressEvent(self, event):
        key = event.key()
        mods = event.modifiers()
        ctrl = bool(mods & Qt.KeyboardModifier.ControlModifier)
        step = 10 if mods & Qt.KeyboardModifier.ShiftModifier else 1

        if key == Qt.Key.Key_Escape:
            self.close()
        elif key in (Qt.Key.Key_M, Qt.Key.Key_Minus, Qt.Key.Key_Underscore):
            self.minimize()
        elif key == Qt.Key.Key_R and not ctrl:
            self.request_retry()
        elif key == Qt.Key.Key_E and not ctrl:
            self.request_edit()
        elif key == Qt.Key.Key_Space and not event.isAutoRepeat():
            self.set_peek(True)
        elif key == Qt.Key.Key_C and ctrl:
            # 划选了就只复制选中的，没划选就复制整块
            picked = self.selected_text()
            QApplication.clipboard().setText(picked or self.translated_text)
        elif key == Qt.Key.Key_A and ctrl:
            QApplication.clipboard().setText(self.translated_text)
        elif key == Qt.Key.Key_Left:
            self._nudge(QPoint(-step, 0))
        elif key == Qt.Key.Key_Right:
            self._nudge(QPoint(step, 0))
        elif key == Qt.Key.Key_Up:
            self._nudge(QPoint(0, -step))
        elif key == Qt.Key.Key_Down:
            self._nudge(QPoint(0, step))
        elif key == Qt.Key.Key_Home:
            self.reset_geometry()
        else:
            super().keyPressEvent(event)

    def keyReleaseEvent(self, event):
        if event.key() == Qt.Key.Key_Space and not event.isAutoRepeat():
            self.set_peek(False)

    # 注意：这里刻意没有「窗口失去焦点就自动关闭」。
    # 那样一来点一下别的窗口结果就没了，和「点任意处关闭」一样容易误触。
    # 想让它让路就点「—」缩到托盘，想关掉就点「✕」或按 Esc。

    def closeEvent(self, event):
        self._spin.stop()
        self._handle_anim.stop()
        self._stop_auto_close()
        if self._bar is not None:
            self._bar.close()
        self.closed.emit()
        super().closeEvent(event)
