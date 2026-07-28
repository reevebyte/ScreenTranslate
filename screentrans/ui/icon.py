"""托盘图标。直接画出来，省得带一个二进制资源文件。

做成一个「对话气泡里的译字」：既是这个程序在干的事（把一段话换个语言说出来），
圆气泡加一条小尾巴也比一个方角色块讨喜。
颜色跟着强调色走，用户在设置里换色，托盘图标下次启动就跟着变。
"""
from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import (QColor, QFont, QIcon, QLinearGradient, QPainter,
                           QPainterPath, QPixmap, QPolygonF)


def _bubble(body: QRectF, s: float, tiny: bool) -> QPainterPath:
    """气泡轮廓，**一笔画完**。

    上一版是「圆角矩形 + 一个三角形」放进同一个 QPainterPath 再 simplified()。
    坑在于 QPainterPath 默认的填充规则是 **OddEvenFill**：两个子图形重叠的那块
    被算了两次，于是**被挖成了一个洞**。画出来就是尾巴根部缺一块背景色的楔子，
    尾巴看着像一面单独歪贴上去的小旗——也就是「左下角错位」。
    （硬要拼图形的话，得先 setFillRule(WindingFill) 再 simplified()。）

    这里索性不拼：左下角不倒角，左边那条边一路直下收成尖，
    尾巴本来就是同一条轮廓的一段，没有重叠、也没有填充规则可踩。
    """
    x0, y0 = body.left(), body.top()
    x1, y1 = body.right(), body.bottom()
    r = (14 if tiny else 15) * s

    path = QPainterPath(QPointF(x0 + r, y0))
    path.lineTo(x1 - r, y0)
    path.quadTo(x1, y0, x1, y0 + r)                     # 右上
    path.lineTo(x1, y1 - r)
    path.quadTo(x1, y1, x1 - r, y1)                     # 右下
    if tiny:
        path.lineTo(x0 + r, y1)
        path.quadTo(x0, y1, x0, y1 - r)                 # 小尺寸没尾巴，四角都倒
    else:
        # 底边往左走到尾巴根，斜下去收成尖，再沿着左边框直接上来
        path.lineTo(x0 + 17 * s, y1)
        path.quadTo(x0 + 6 * s, y1 + 3 * s, x0 + 0.5 * s, y1 + 11 * s)   # 尖
        path.quadTo(x0, y1 + 9 * s, x0, y1 - 3 * s)
    path.lineTo(x0, y0 + r)
    path.quadTo(x0, y0, x0 + r, y0)                     # 左上
    path.closeSubpath()
    return path


def _draw(p: QPainter, size: int, accent: str) -> None:
    s = size / 64.0                        # 所有尺寸都按 64 的网格写，再乘这个系数
    base = QColor(accent)
    # 小到 20px 以下就把尾巴收掉，气泡撑满整格。
    # 「译」这个字笔画不少，16px 图标里如果还留出尾巴的位置，字只剩 7px 高，
    # 出来就是一坨绿色上抹了道黑——托盘里最常见的恰恰是 16px。
    tiny = size < 22
    body = (QRectF(1 * s, 1 * s, 62 * s, 62 * s) if tiny
            else QRectF(4 * s, 2 * s, 56 * s, 49 * s))

    # 气泡：上浅下深的一点点渐变，纯色块会显得很平
    grad = QLinearGradient(body.topLeft(), body.bottomLeft())
    grad.setColorAt(0.0, base.lighter(118))
    grad.setColorAt(1.0, base)

    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(grad)
    p.drawPath(_bubble(body, s, tiny))

    font = QFont("Microsoft YaHei UI")
    font.setPixelSize(max(9, int((44 if tiny else 31) * s)))
    font.setBold(True)
    p.setFont(font)
    # 字压深一点而不是纯白：亮底上纯白字会发飘，深字反而更清楚
    p.setPen(QColor(12, 22, 16, 240) if base.lightnessF() > 0.55 else QColor(255, 255, 255, 245))
    p.drawText(body.adjusted(0, (0 if tiny else -1) * s, 0, (0 if tiny else -1) * s),
               Qt.AlignmentFlag.AlignCenter, "译")


def make_icon(accent: str = "#28C76F", size: int = 64) -> QIcon:
    ico = QIcon()
    # 托盘会按 DPI 挑不同尺寸，一次都给了，省得系统去缩放糊掉
    for px in sorted({16, 20, 24, 32, size}):
        pix = QPixmap(px, px)
        pix.fill(Qt.GlobalColor.transparent)
        p = QPainter(pix)
        p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        p.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        _draw(p, px, accent)
        p.end()
        ico.addPixmap(pix)
    return ico
