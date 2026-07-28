"""全程序共用的一套图标。每一个都是画出来的，没有一个是图片文件。

为什么不从网上拿现成的图标包：

  1. **授权**。图标包基本都带许可证，打进 exe 分发就得连许可证一起带，
     还得留意有没有「不得再分发」的条款。自己画的没这个包袱。
  2. **少一处失败点**。多一个二进制资源，就多一次「打包漏了 → 界面上一片空白」，
     而且这种错 PyInstaller 不会报，只会静悄悄地少东西。
  3. **本来就不齐**。不同来源的图标线宽、圆角、留白都不一样，凑在一起反而更乱。
     自己画的全都跑同一套参数：24 格网格、线宽 STROKE、圆头圆角，
     放在一起天生就是一套。

风格是有意往圆润里做的：线头线尾都是圆的、拐角是圆的、形状偏胖，
比尖角细线的那种「工程感」图标讨喜一些。

用法：
    glyphs.paint(painter, "close", rect, color)   # 画进任意矩形
    btn.setIcon(glyphs.icon("gear", 18, "#9AA0A9"))
"""
from __future__ import annotations

import math

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import (QColor, QIcon, QPainter, QPainterPath, QPen,
                           QPixmap, QPolygonF)

GRID = 24.0          # 所有图标都按 24×24 的网格画，再缩放到实际大小
STROKE = 2.0         # 统一线宽（网格单位）


# ------------------------------------------------------------------ 画笔工具

def _pen(p: QPainter, color: QColor, w: float) -> QPen:
    pen = QPen(color, w)
    pen.setCapStyle(Qt.PenCapStyle.RoundCap)
    pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
    p.setPen(pen)
    p.setBrush(Qt.BrushStyle.NoBrush)
    return pen


def _line(p, x1, y1, x2, y2):
    p.drawLine(QPointF(x1, y1), QPointF(x2, y2))


def _poly(p, pts):
    p.drawPolyline(QPolygonF([QPointF(x, y) for x, y in pts]))


# ---------------------------------------------------------------- 各个图标
# 每个函数都在 24×24 的网格里画，颜色和线宽由外面设好。

def _close(p, c, w):
    _pen(p, c, w)
    _line(p, 7.5, 7.5, 16.5, 16.5)
    _line(p, 16.5, 7.5, 7.5, 16.5)


def _minus(p, c, w):
    _pen(p, c, w)
    _line(p, 7, 12, 17, 12)


def _retry(p, c, w):
    """缺口朝上的圆圈 + 一个箭头 = 再来一次。"""
    r = 6.4
    cx = cy = 12.0
    _pen(p, c, w)
    # Qt 的角度：0 度在 3 点方向，正数逆时针。从 105 度扫 300 度，缺口留在正上方偏右
    p.drawArc(QRectF(cx - r, cy - r, r * 2, r * 2), 105 * 16, 300 * 16)
    a = math.radians(105)                       # 箭头接在缺口左侧那一端
    px, py = cx + r * math.cos(a), cy - r * math.sin(a)
    tx, ty = math.sin(a), math.cos(a)           # 顺时针方向的切线，箭头朝这边
    nx, ny = -ty, tx                            # 法线，用来撑开箭头底边
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(c)
    p.drawPolygon(QPolygonF([
        QPointF(px + tx * 4.2, py + ty * 4.2),
        QPointF(px - tx * 0.8 + nx * 2.9, py - ty * 0.8 + ny * 2.9),
        QPointF(px - tx * 0.8 - nx * 2.9, py - ty * 0.8 - ny * 2.9),
    ]))


def _edit(p, c, w):
    """斜放的铅笔；小尺寸下保留清楚的笔尖和尾盖。"""
    _pen(p, c, w)
    body = QPolygonF([
        QPointF(5.2, 15.4),
        QPointF(14.8, 5.8),
        QPointF(18.2, 9.2),
        QPointF(8.6, 18.8),
        QPointF(4.2, 19.8),
    ])
    p.drawPolyline(body)
    _line(p, 5.2, 15.4, 8.6, 18.8)
    _line(p, 13.3, 7.3, 16.7, 10.7)


def _check(p, c, w):
    _pen(p, c, w)
    _poly(p, [(5.8, 12.4), (10.2, 16.6), (18.2, 7.6)])


def _chevron(p, c, w):
    """朝下的小尖角。比实心三角更轻，也跟其他图标是同一种线。"""
    _pen(p, c, w)
    _poly(p, [(7.5, 10), (12, 14.6), (16.5, 10)])


def _keyboard(p, c, w):
    """外框 + 一排按键点 + 一根空格。

    按键点必须用实心圆画。一开始写成零长度的线想借圆头笔帽当点，
    结果直径只有线宽那么大，16px 下整排键全糊没了，只剩一个空框。
    """
    _pen(p, c, w)
    p.drawRoundedRect(QRectF(2.4, 6.2, 19.2, 11.6), 2.8, 2.8)
    _line(p, 8.4, 14.6, 15.6, 14.6)             # 空格
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(c)
    for x in (6.5, 10.2, 13.8, 17.5):
        p.drawEllipse(QPointF(x, 10.3), 0.95, 0.95)


def _globe(p, c, w):
    """地球：一眼就是「语言 / 翻译」。"""
    _pen(p, c, w)
    p.drawEllipse(QRectF(3.4, 3.4, 17.2, 17.2))
    _line(p, 3.4, 12, 20.6, 12)
    # 竖着的那条经线用两段弧拼，比一个扁椭圆更像球面
    path = QPainterPath(QPointF(12, 3.4))
    path.cubicTo(16.4, 7.2, 16.4, 16.8, 12, 20.6)
    path.cubicTo(7.6, 16.8, 7.6, 7.2, 12, 3.4)
    p.drawPath(path)


def _scan(p, c, w):
    """四个角括号 + 中间一条扫描线 = 文字识别。"""
    _pen(p, c, w)
    for sx, sy in ((1, 1), (-1, 1), (1, -1), (-1, -1)):
        ox = 12 + sx * -7.6
        oy = 12 + sy * -7.6
        _poly(p, [(ox + sx * 0.0, oy + sy * 3.4),
                  (ox, oy),
                  (ox + sx * 3.4, oy)])
    _line(p, 5.4, 12, 18.6, 12)


def _contrast(p, c, w):
    """半黑半白的圆 = 外观 / 显示。"""
    _pen(p, c, w)
    p.drawEllipse(QRectF(3.6, 3.6, 16.8, 16.8))
    path = QPainterPath(QPointF(12, 3.6))
    path.arcTo(QRectF(3.6, 3.6, 16.8, 16.8), 90, 180)
    path.closeSubpath()
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(c)
    p.drawPath(path)


def _sliders(p, c, w):
    """三根带旋钮的横杆 = 设置里的杂项。旋钮画成实心点，比齿轮在小尺寸下清楚。"""
    _pen(p, c, w)
    for y, knob in ((7.0, 15.0), (12.0, 9.4), (17.0, 16.0)):
        _line(p, 4.4, y, 19.6, y)
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(c)
    for y, knob in ((7.0, 15.0), (12.0, 9.4), (17.0, 16.0)):
        p.drawEllipse(QPointF(knob, y), w * 1.25, w * 1.25)


def _copy(p, c, w):
    _pen(p, c, w)
    p.drawRoundedRect(QRectF(8.4, 3.6, 12.0, 12.0), 2.6, 2.6)
    _poly(p, [(15.6, 20.4), (5.2, 20.4), (5.2, 10.0)])


def _download(p, c, w):
    _pen(p, c, w)
    _line(p, 12, 3.8, 12, 14.4)
    _poly(p, [(7.8, 10.5), (12, 14.8), (16.2, 10.5)])
    _poly(p, [(5.2, 17.2), (5.2, 20.2), (18.8, 20.2), (18.8, 17.2)])


def _eye(p, c, w):
    _pen(p, c, w)
    path = QPainterPath(QPointF(2.6, 12))
    path.cubicTo(6.6, 5.6, 17.4, 5.6, 21.4, 12)
    path.cubicTo(17.4, 18.4, 6.6, 18.4, 2.6, 12)
    p.drawPath(path)
    p.drawEllipse(QPointF(12.0, 12.0), 2.8, 2.8)


def _info(p, c, w):
    _pen(p, c, w)
    p.drawEllipse(QRectF(3.4, 3.4, 17.2, 17.2))
    _line(p, 12, 11.0, 12, 16.6)
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(c)
    p.drawEllipse(QPointF(12.0, 7.6), w * 0.62, w * 0.62)


_GLYPHS = {
    "close": _close,
    "minus": _minus,
    "retry": _retry,
    "edit": _edit,
    "check": _check,
    "chevron": _chevron,
    "keyboard": _keyboard,
    "globe": _globe,
    "scan": _scan,
    "contrast": _contrast,
    "sliders": _sliders,
    # 齿轮试过了，删掉了：16px 下八个齿糊成一圈毛边，看着像太阳不像齿轮。
    # 「设置」这个意思交给 sliders，小尺寸下清楚得多。
    "copy": _copy,
    "download": _download,
    "eye": _eye,
    "info": _info,
}


def names() -> list[str]:
    return sorted(_GLYPHS)


# ------------------------------------------------------------------ 对外接口

def paint(p: QPainter, name: str, rect: QRectF, color, weight: float = STROKE) -> None:
    """把图标画进 rect。24 格网格会等比缩放并居中，所以 rect 不是正方形也不会变形。"""
    fn = _GLYPHS.get(name)
    if fn is None:
        return
    s = min(rect.width(), rect.height()) / GRID
    if s <= 0:
        return
    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    p.translate(rect.center().x() - GRID * s / 2, rect.center().y() - GRID * s / 2)
    p.scale(s, s)
    # 线宽跟着缩放走，但太小的时候补一点，免得细到看不见
    w = weight if s >= 0.62 else weight * (0.62 / s) ** 0.45
    fn(p, QColor(color), w)
    p.restore()


def pixmap(name: str, size: int, color, dpr: float = 1.0,
           weight: float = STROKE) -> QPixmap:
    pix = QPixmap(int(size * dpr), int(size * dpr))
    pix.setDevicePixelRatio(dpr)
    pix.fill(Qt.GlobalColor.transparent)
    p = QPainter(pix)
    paint(p, name, QRectF(0, 0, size, size), color, weight)
    p.end()
    return pix


def icon(name: str, size: int, color, weight: float = STROKE) -> QIcon:
    ico = QIcon()
    # 两档分辨率，高 DPI 屏上才不糊
    for dpr in (1.0, 2.0):
        ico.addPixmap(pixmap(name, size, color, dpr, weight))
    return ico
