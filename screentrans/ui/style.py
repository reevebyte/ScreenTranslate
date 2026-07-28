"""设置界面的样式表。刻意做得很克制：深色、只有一个强调色、圆角只用两档。

样式表里出现的所有小图形（对勾、下拉箭头）都从 `glyphs` 来，
跟托盘菜单、控制条上的图标是同一套线宽和圆角。
"""
from __future__ import annotations

ACCENT = "#28C76F"
CHECK_PX = 16          # 复选框方块边长，对勾图必须画成同样大小
# 图形改版时把它 +1。这些 png 会缓存在配置目录里，同名文件存在就不重画，
# 不换名字的话老用户永远看到旧图标——改了等于没改。
ASSET_REV = 2


def _asset(name: str, size: int, painter) -> str:
    """Qt 样式表里用 border 拼三角形、拼对勾都不可靠，干脆自己画好存成小图。

    图按原始尺寸贴、不缩放，所以 size 必须和样式表里那个部件的尺寸一致。
    """
    from PySide6.QtCore import QRectF, Qt
    from PySide6.QtGui import QPainter, QPixmap

    from ..config import CONFIG_DIR

    path = CONFIG_DIR / f"{name}{ASSET_REV}.png"
    if not path.exists():
        try:
            CONFIG_DIR.mkdir(parents=True, exist_ok=True)
            # 存两倍大小再让 Qt 缩：高 DPI 屏上 1 倍图会糊
            pix = QPixmap(size * 2, size * 2)
            pix.fill(Qt.GlobalColor.transparent)
            p = QPainter(pix)
            painter(p, QRectF(0, 0, size * 2, size * 2))
            p.end()
            pix.save(str(path))
        except Exception:
            return ""
    return path.as_posix()


def _arrow_asset() -> str:
    from . import glyphs

    return _asset("arrow", 18, lambda p, r: glyphs.paint(p, "chevron", r, "#8A8F98", 3.4))


def _check_asset() -> str:
    """对勾。没有它的话，勾上的复选框只是一个色块，看不出「打上勾了」。"""
    from . import glyphs

    return _asset("check", CHECK_PX, lambda p, r: glyphs.paint(p, "check", r, "#FFFFFF", 3.6))


def build_qss(accent: str = ACCENT) -> str:
    arrow = _arrow_asset()
    check = _check_asset()
    return QSS_TEMPLATE.replace("@ACCENT@", accent).replace(
        "@ACCENT_HI@", _lighten(accent, 1.16)
    ) + f"""
QComboBox::down-arrow {{
    {f"image: url({arrow});" if arrow else "image: none;"}
    width: 18px;
    height: 18px;
}}
QCheckBox::indicator:checked {{
    {f"image: url({check});" if check else ""}
}}
"""


def _lighten(hex_color: str, factor: float) -> str:
    from PySide6.QtGui import QColor

    c = QColor(hex_color)
    return (c.lighter(int(factor * 100)).name() if c.isValid() else hex_color)


# 颜色只用这一套，别在别处再拍脑袋写十六进制。
# 层次是「越靠上越亮」：桌面 → 侧栏 → 页面 → 卡片 → 输入框。
BG_SIDE = "#101116"
BG_PAGE = "#15161B"
BG_CARD = "#1B1D23"
BG_INPUT = "#212429"
LINE = "#282B33"
LINE_HI = "#343842"
TEXT = "#E7E9EC"
TEXT_DIM = "#949AA4"      # 说明文字。原来的 #7E838C 在卡片底上对比度只有 3.2，偏灰
TEXT_FAINT = "#6E747E"
OK_GREEN = "#4ED18B"
WARN = "#E8B44A"
BAD = "#FF6B6B"

# 圆角只有两档：小件 6、卡片 10。档位一多，界面就显得没规矩。
R_SM = "6px"
R_LG = "10px"

QSS_TEMPLATE = f"""
QWidget#Root {{
    background: {BG_PAGE};
    color: {TEXT};
}}
QWidget {{
    font-family: "Microsoft YaHei UI", "Segoe UI", sans-serif;
    font-size: 13px;
    color: {TEXT};
}}

/* ------------------------------------------------------------ 左侧导航 */
QWidget#Sidebar {{
    background: {BG_SIDE};
    border-right: 1px solid {LINE};
}}
QLabel#BrandTitle {{
    color: {TEXT};
    font-size: 14px;
    font-weight: 600;
}}
QLabel#BrandSub {{
    color: {TEXT_FAINT};
    font-size: 11px;
    letter-spacing: 3px;
}}
QListWidget#Nav {{
    background: transparent;
    border: none;
    outline: none;
    padding: 0 10px;
}}
QListWidget#Nav::item {{
    color: #9AA0A9;
    padding: 0 10px;
    border-radius: {R_SM};
    margin: 2px 0;
}}
QListWidget#Nav::item:hover {{
    background: #1B1E25;
    color: #C9CDD4;
}}
QListWidget#Nav::item:selected {{
    background: #23272F;
    color: #FFFFFF;
}}
QLabel#SideFoot {{
    color: {TEXT_FAINT};
    font-size: 11px;
}}
QLabel#SaveError {{
    background: #3A2024;
    color: {BAD};
    border-bottom: 1px solid #71343C;
    padding: 9px 14px;
}}

/* ---------------------------------------------------------------- 页面 */
QLabel#PageTitle {{
    color: #F2F3F6;
    font-size: 18px;
    font-weight: 600;
}}
QLabel#PageSub {{
    color: {TEXT_DIM};
    font-size: 12px;
}}
QLabel#SectionTitle {{
    color: {TEXT_DIM};
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 1px;
    padding: 8px 0 0 2px;
}}
QLabel#Hint {{
    color: {TEXT_DIM};
    font-size: 11px;
}}
QLabel#Status {{
    color: {TEXT_DIM};
    font-size: 11px;
}}
QLabel#Key {{
    color: #C9CDD4;
    font-size: 11px;
    font-weight: 600;
}}
QLabel#Code {{
    background: #101216;
    border: 1px solid {LINE};
    border-radius: {R_SM};
    color: #9FD8B4;
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 12px;
    padding: 8px 11px;
}}
QFrame#Card {{
    background: {BG_CARD};
    border: 1px solid {LINE};
    border-radius: {R_LG};
}}
QFrame#Sep {{
    background: {LINE};
    max-height: 1px;
    border: none;
}}

/* -------------------------------------------------------------- 输入件 */
QLineEdit, QComboBox, QSpinBox {{
    background: {BG_INPUT};
    border: 1px solid {LINE_HI};
    border-radius: {R_SM};
    padding: 7px 9px;
    selection-background-color: @ACCENT@;
    selection-color: #FFFFFF;
}}
QLineEdit:hover, QComboBox:hover, QSpinBox:hover {{
    border: 1px solid #414651;
}}
QLineEdit:focus, QComboBox:focus, QSpinBox:focus {{
    border: 1px solid @ACCENT@;
}}
QLineEdit:read-only {{
    background: #191B20;
}}
QComboBox::drop-down {{
    border: none;
    width: 22px;
}}
QComboBox QAbstractItemView {{
    background: {BG_INPUT};
    border: 1px solid {LINE_HI};
    border-radius: {R_SM};
    selection-background-color: @ACCENT@;
    selection-color: #FFFFFF;
    outline: none;
    padding: 3px;
}}
QSpinBox::up-button, QSpinBox::down-button {{
    width: 0;
    border: none;
}}

/* 托盘右键菜单。必须显式给背景色：QWidget 的规则只改了文字色，
   菜单背景会沿用系统浅色主题，浅底浅字等于看不见。 */
QMenu {{
    background: {BG_CARD};
    border: 1px solid {LINE_HI};
    border-radius: {R_SM};
    padding: 6px;
}}
QMenu::item {{
    background: transparent;
    color: {TEXT};
    padding: 7px 28px 7px 12px;
    border-radius: 5px;
    margin: 1px 2px;
}}
QMenu::item:selected {{
    background: @ACCENT@;
    color: #FFFFFF;
}}
QMenu::item:disabled {{
    color: {TEXT_FAINT};
}}
QMenu::separator {{
    height: 1px;
    background: {LINE};
    margin: 5px 8px;
}}
QMenu::icon {{
    padding-left: 8px;
}}
QToolTip {{
    background: #1B1D23;
    color: {TEXT};
    border: 1px solid {LINE_HI};
    border-radius: 5px;
    padding: 5px 8px;
}}

/* -------------------------------------------------------------- 按钮 */
QPushButton {{
    background: #282C34;
    border: 1px solid #363B45;
    border-radius: {R_SM};
    padding: 7px 15px;
}}
QPushButton:hover {{
    background: #323741;
    border: 1px solid #434955;
}}
QPushButton:pressed {{
    background: #23262D;
}}
QPushButton:disabled {{
    background: #202329;
    color: {TEXT_FAINT};
    border: 1px solid #2C3038;
}}
QPushButton#Primary {{
    background: @ACCENT@;
    border: 1px solid @ACCENT@;
    color: #0E1013;
    font-weight: 600;
}}
QPushButton#Primary:hover {{
    background: @ACCENT_HI@;
    border: 1px solid @ACCENT_HI@;
}}
QPushButton#Ghost {{
    background: transparent;
    border: 1px solid {LINE_HI};
    color: {TEXT_DIM};
}}
QPushButton#Ghost:hover {{
    background: #1F2229;
    color: {TEXT};
}}

QCheckBox {{
    spacing: 9px;
}}
QCheckBox::indicator {{
    width: {CHECK_PX}px;
    height: {CHECK_PX}px;
    border: 1px solid #3C414B;
    border-radius: 4px;
    background: {BG_INPUT};
}}
QCheckBox::indicator:hover {{
    border: 1px solid #4C525E;
}}
QCheckBox::indicator:checked {{
    background: @ACCENT@;
    border: 1px solid @ACCENT@;
}}

QScrollArea {{
    border: none;
    background: transparent;
}}
QScrollBar:vertical {{
    background: transparent;
    width: 10px;
    margin: 0;
}}
QScrollBar::handle:vertical {{
    background: #33373F;
    border-radius: 5px;
    min-height: 34px;
}}
QScrollBar::handle:vertical:hover {{
    background: #454B57;
}}
QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical,
QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical {{
    height: 0;
    background: none;
}}
"""
