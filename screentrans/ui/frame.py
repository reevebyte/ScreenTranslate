"""选区边框：框选遮罩和译文窗口共用同一套画法，看起来才是同一个东西。

比原来那条 1px、半透明的细线明显好认：
  · 边框加粗到 2px，外面再垫一圈半透明黑。只有一条彩线的话，
    落在浅色背景上（白底文档、浅色网页）几乎看不见，垫一圈暗边两种底都清楚。
  · 八个角点/边点画成实心小方块。原来四条边和四个角都能拖，但**一点提示都没有**，
    得靠鼠标划过去变光标才发现——等于藏起来的功能。
  · 拖动时在角上贴一个「宽 × 高」的小牌子。
"""
from __future__ import annotations

from PySide6.QtCore import QPointF, QRect, QRectF, Qt
from PySide6.QtGui import QColor, QFont, QFontMetrics, QPainter, QPen

BORDER = 2           # 边框粗细（逻辑像素）
HANDLE = 8           # 手柄方块边长
# 小于这个尺寸就不画手柄了：一个 40×20 的选区再摆八个 8px 的方块，
# 方块本身就把内容盖住了，反而看不清框的是什么。
MIN_FOR_HANDLES = 56, 34

# 手柄的方位 -> 在矩形里的相对位置（0=左/上，0.5=中，1=右/下）
ZONES = {
    "tl": (0.0, 0.0), "t": (0.5, 0.0), "tr": (1.0, 0.0),
    "l":  (0.0, 0.5), "r": (1.0, 0.5),
    "bl": (0.0, 1.0), "b": (0.5, 1.0), "br": (1.0, 1.0),
}


def handle_rects(rect: QRect, size: int = HANDLE) -> dict[str, QRectF]:
    """八个手柄各自占的方块。全部画在框**里面**——译文窗口的大小就是选区的大小，
    一个像素都长不到外面去，跨在边线上画会被裁掉半个。"""
    out: dict[str, QRectF] = {}
    for zone, (fx, fy) in ZONES.items():
        x = rect.left() + (rect.width() - size) * fx
        y = rect.top() + (rect.height() - size) * fy
        out[zone] = QRectF(x, y, size, size)
    return out


def draw(p: QPainter, rect: QRect, accent: QColor, handles: float = 1.0) -> None:
    """画边框和手柄。handles 是手柄的不透明度，0 就只画边框。"""
    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    p.setBrush(Qt.BrushStyle.NoBrush)

    outer = QRectF(rect).adjusted(0.5, 0.5, -0.5, -0.5)
    # 外面这圈暗边是为了让彩色边框在浅色背景上也能看见
    p.setPen(QPen(QColor(0, 0, 0, 90), 1))
    p.drawRect(outer)
    inner = outer.adjusted(BORDER - 0.5, BORDER - 0.5, -(BORDER - 0.5), -(BORDER - 0.5))
    if inner.width() > 0 and inner.height() > 0:
        p.drawRect(inner)

    pen = QPen(accent, BORDER)
    pen.setJoinStyle(Qt.PenJoinStyle.MiterJoin)
    p.setPen(pen)
    p.drawRect(QRectF(rect).adjusted(BORDER / 2, BORDER / 2, -BORDER / 2, -BORDER / 2))

    if handles > 0.01 and rect.width() >= MIN_FOR_HANDLES[0] and rect.height() >= MIN_FOR_HANDLES[1]:
        fill = QColor(accent)
        fill.setAlphaF(min(1.0, handles))
        edge = QColor(255, 255, 255, int(215 * min(1.0, handles)))
        p.setBrush(fill)
        p.setPen(QPen(edge, 1))
        for r in handle_rects(rect).values():
            p.drawRoundedRect(r.adjusted(0.5, 0.5, -0.5, -0.5), 1.6, 1.6)
    p.restore()


def draw_size_badge(p: QPainter, rect: QRect, accent: QColor,
                    bounds: QRect | None = None) -> None:
    """在选区左上角外面贴一个「宽 × 高」的小牌子；上面放不下就挪到框里面。"""
    text = f"{rect.width()} × {rect.height()}"
    font = QFont("Microsoft YaHei UI")
    font.setPixelSize(11)
    fm = QFontMetrics(font)
    w = fm.horizontalAdvance(text) + 14
    h = fm.height() + 7

    x = rect.left()
    y = rect.top() - h - 6
    if bounds is not None:
        if y < bounds.top():
            y = rect.top() + 6           # 上面没地方就贴在框里
        x = max(bounds.left(), min(x, bounds.right() + 1 - w))

    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(QColor(18, 20, 24, 232))
    p.drawRoundedRect(QRectF(x, y, w, h), h / 2, h / 2)
    p.setFont(font)
    p.setPen(QColor(accent).lighter(118))
    p.drawText(QRectF(x, y, w, h), Qt.AlignmentFlag.AlignCenter, text)
    p.restore()
