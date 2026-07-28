"""设置窗口。左边一列导航，右边一页内容；改哪一项就立刻存哪一项，没有「保存」按钮。"""
from __future__ import annotations

import subprocess
import time

from PySide6.QtCore import QEvent, QObject, QSize, Qt, Signal
from PySide6.QtGui import QFont, QFontDatabase
from PySide6.QtWidgets import (
    QAbstractScrollArea,
    QAbstractSpinBox,
    QCheckBox,
    QComboBox,
    QFontComboBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSpinBox,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from .. import translators
from ..background import DaemonWorker
from ..config import APP_TITLE, CONFIG_PATH, HOTKEYS
from ..hotkey import parse as parse_hotkey
from ..network import configured_secrets, redact_sensitive
from ..ocr import available_engines
from ..ocr import windows_ocr
from . import glyphs
from .hotkey_edit import HotkeyEdit
from .icon import make_icon
from .style import build_qss
from .swatch import SwatchRow

SHUTDOWN_WAIT_MS = 2500
LABEL_W = 84          # 所有「标签 : 控件」行的标签宽度，统一了才对得齐
NAV_ICON = 17         # 左侧导航的图标大小

_ZH_TARGETS = [
    ("en", "英语"),
    ("ja", "日语"),
    ("ko", "韩语"),
    ("fr", "法语"),
    ("de", "德语"),
    ("es", "西班牙语"),
    ("ru", "俄语"),
    ("zh-Hant", "繁体中文"),
]

_CLOSE_MODES = [
    ("click", "只在点 ✕ 或按 Esc 时"),   # 配置值沿用 click，免得旧配置失效
    ("timeout", "过几秒自动消失"),
    ("leave", "鼠标移开就消失"),
]

_OCR_LANG_NAMES = {
    "zh-Hans-CN": "简体中文", "zh-Hant-TW": "繁体中文", "zh-Hant-HK": "繁体中文（港）",
    "en-US": "英语", "en-GB": "英语（英）", "ja": "日语", "ko": "韩语",
    "fr-FR": "法语", "de-DE": "德语", "es-ES": "西班牙语", "ru-RU": "俄语",
    "it-IT": "意大利语", "pt-BR": "葡萄牙语",
}


def _lang_name(tag: str) -> str:
    return _OCR_LANG_NAMES.get(tag, tag)


class _WheelGuard(QObject):
    """让滚轮只滚页面，不去改下拉框和数字框的值。

    Qt 默认：鼠标滚到下拉框上面，滚轮就直接切换选项了——想往下翻页却把设置改了。
    这里把没有焦点时的滚轮事件转交给它所在的滚动区域，只有点进去（获得焦点）
    之后滚轮才作用于控件本身。每页各有一个滚动区域，所以要顺着父级往上找，
    不能记死一个。
    """

    @staticmethod
    def _area(widget) -> QAbstractScrollArea | None:
        node = widget.parentWidget()
        while node is not None:
            if isinstance(node, QAbstractScrollArea):
                return node
            node = node.parentWidget()
        return None

    def eventFilter(self, obj, event):
        if event.type() == QEvent.Type.Wheel and not obj.hasFocus():
            area = self._area(obj)
            if area is None:
                return super().eventFilter(obj, event)
            bar = area.verticalScrollBar()
            # 按 Qt 的默认步长换算：一格滚轮 = 120，滚 3 个单位步长
            bar.setValue(bar.value() - int(event.angleDelta().y() / 120 * bar.singleStep() * 3))
            return True
        return super().eventFilter(obj, event)

    def guard(self, widget):
        widget.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        widget.installEventFilter(self)
        return widget


class _TestThread(DaemonWorker):
    done = Signal(bool, str)

    def __init__(self, provider: str, opts: dict):
        super().__init__(thread_name="ScreenTranslate-ConnectionTest")
        self._provider = provider
        self._opts = dict(opts)

    def run(self):
        engine = None
        try:
            engine = translators.build(self._provider, self._opts)
            engine.cancel_check = self.is_cancelled
            message = engine.check()
            if not self.cancelled:
                self.done.emit(True, message)
        except Exception as exc:
            if not self.cancelled:
                self.done.emit(
                    False,
                    translators.friendly(exc, configured_secrets(self._opts)),
                )
        finally:
            if engine is not None:
                close = getattr(engine, "close", None)
                if callable(close):
                    close()


class _ModelsThread(DaemonWorker):
    done = Signal(bool, object)   # (成功?, 模型名列表 或 错误信息)

    def __init__(self, provider: str, opts: dict):
        super().__init__(thread_name="ScreenTranslate-ModelList")
        self._provider = provider
        self._opts = dict(opts)

    def run(self):
        engine = None
        try:
            engine = translators.build(self._provider, self._opts)
            engine.cancel_check = self.is_cancelled
            models = engine.list_models()
            if not self.cancelled:
                self.done.emit(True, models)
        except Exception as exc:
            if not self.cancelled:
                self.done.emit(
                    False,
                    translators.friendly(exc, configured_secrets(self._opts)),
                )
        finally:
            if engine is not None:
                close = getattr(engine, "close", None)
                if callable(close):
                    close()


class SettingsWindow(QWidget):
    hotkeyEdited = Signal(str, str)      # (内部名, 组合键)
    settingsSaved = Signal()
    restartRequested = Signal()

    def __init__(self, cfg):
        super().__init__()
        self.cfg = cfg
        self._test_thread: _TestThread | None = None
        self._test_provider: str | None = None
        self._models_thread: _ModelsThread | None = None
        self._models_provider: str | None = None
        self._provider_fields: dict[str, QLineEdit] = {}
        self.hotkey_edits: dict[str, HotkeyEdit] = {}
        self.hotkey_status: dict[str, QLabel] = {}
        self._nav_icons: list[tuple[int, str]] = []

        self.setObjectName("Root")
        self.setWindowTitle(f"{APP_TITLE} · 设置")
        self.accent = cfg.get("appearance.accent", "#28C76F")
        self.setStyleSheet(build_qss(self.accent))
        self.resize(790, 610)
        self.setMinimumSize(680, 470)

        self._wheel_guard = _WheelGuard(self)

        outer = QHBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)
        outer.addWidget(self._build_sidebar())

        content = QWidget()
        content.setObjectName("Root")
        content_layout = QVBoxLayout(content)
        content_layout.setContentsMargins(0, 0, 0, 0)
        content_layout.setSpacing(0)

        self.save_status = QLabel("")
        self.save_status.setObjectName("SaveError")
        self.save_status.setWordWrap(True)
        self.save_status.setVisible(False)
        content_layout.addWidget(self.save_status)

        self.stack = QStackedWidget()
        content_layout.addWidget(self.stack, 1)
        outer.addWidget(content, 1)

        self._build_hotkey()
        self._build_translator()
        self._build_ocr()
        self._build_appearance()
        self._build_misc()

        self.nav.currentRowChanged.connect(self.stack.setCurrentIndex)
        self.nav.currentRowChanged.connect(self._sync_nav_icons)
        self.nav.setCurrentRow(0)
        self._guard_wheel()

    # ------------------------------------------------------------ 骨架
    def _build_sidebar(self) -> QWidget:
        side = QWidget()
        side.setObjectName("Sidebar")
        side.setFixedWidth(166)
        lay = QVBoxLayout(side)
        lay.setContentsMargins(0, 18, 0, 12)
        lay.setSpacing(0)

        brand = QWidget()
        brow = QHBoxLayout(brand)
        brow.setContentsMargins(16, 0, 14, 0)
        brow.setSpacing(9)
        mark = QLabel()
        mark.setPixmap(make_icon(self.accent, 26).pixmap(26, 26))
        mark.setFixedSize(26, 26)
        brow.addWidget(mark)
        names = QVBoxLayout()
        names.setContentsMargins(0, 0, 0, 0)
        names.setSpacing(0)
        title = QLabel(APP_TITLE)
        title.setObjectName("BrandTitle")
        names.addWidget(title)
        sub = QLabel("设置")
        sub.setObjectName("BrandSub")
        names.addWidget(sub)
        brow.addLayout(names, 1)
        lay.addWidget(brand)
        lay.addSpacing(16)

        self.nav = QListWidget()
        self.nav.setObjectName("Nav")
        self.nav.setFrameShape(QFrame.Shape.NoFrame)
        self.nav.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.nav.setIconSize(QSize(NAV_ICON, NAV_ICON))
        lay.addWidget(self.nav, 1)

        # 底下常驻显示框选快捷键——用的最多的就这一个，不该还要点进「快捷键」页才看得到
        self.side_foot = QLabel()
        self.side_foot.setObjectName("SideFoot")
        self.side_foot.setContentsMargins(16, 0, 12, 0)
        self.side_foot.setToolTip(f"配置文件：{CONFIG_PATH}")
        lay.addWidget(self.side_foot)
        self._refresh_side_foot()
        return side

    def _refresh_side_foot(self) -> None:
        spec = self.cfg.get("hotkey", "Ctrl+Alt+Q")
        self.side_foot.setText(f"框选  {spec}")

    def _page(self, name: str, subtitle: str, icon: str = "") -> QVBoxLayout:
        """新建一页，返回往里塞卡片用的竖排布局。"""
        area = QScrollArea()
        area.setWidgetResizable(True)
        area.setFrameShape(QFrame.Shape.NoFrame)

        body = QWidget()
        body.setObjectName("Root")
        column = QVBoxLayout(body)
        column.setContentsMargins(28, 24, 28, 24)
        column.setSpacing(12)

        head = QLabel(name)
        head.setObjectName("PageTitle")
        column.addWidget(head)
        if subtitle:
            note = QLabel(subtitle)
            note.setObjectName("PageSub")
            note.setWordWrap(True)
            column.addWidget(note)
        column.addSpacing(4)

        area.setWidget(body)
        self.stack.addWidget(area)

        item = QListWidgetItem(name)
        item.setSizeHint(QSize(0, 36))
        if icon:
            # 选中态是白字，未选中是灰字；图标跟着换一版，不然选中行的图标会发暗
            item.setIcon(glyphs.icon(icon, NAV_ICON, "#A6ACB6"))
            self._nav_icons.append((self.nav.count(), icon))
        self.nav.addItem(item)

        self.column = column
        return column

    def _sync_nav_icons(self, row: int) -> None:
        for i, name in self._nav_icons:
            color = "#FFFFFF" if i == row else "#A6ACB6"
            self.nav.item(i).setIcon(glyphs.icon(name, NAV_ICON, color))

    def _save(self) -> bool:
        """存盘，但**绝不让存盘失败打断界面**。

        以前这里是直接 cfg.save()。它一旦抛异常（配置文件被杀软/同步盘占住等），
        整个槽函数就在那一行断掉，后面的界面刷新根本轮不到执行——
        表现就是「下拉框已经切到英伟达了，下面还显示着微软的密钥/区域/终结点」。
        存盘失败要说出来，但界面该更新还是得更新。
        """
        try:
            self.cfg.save()
            self._save_error = ""
            self.save_status.clear()
            self.save_status.setVisible(False)
            return True
        except Exception as exc:
            print(f"[settings] 配置写盘失败：{exc}")
            self._save_error = "配置保存失败，刚才的修改尚未写入磁盘。请检查配置目录权限或文件是否被占用。"
            self.save_status.setText(self._save_error)
            self.save_status.setVisible(True)
            return False

    def _guard_wheel(self) -> None:
        """统一处理所有下拉框/数字框的滚轮行为，顺带管住按钮的焦点。

        用遍历而不是逐个调用，是为了以后新增控件时不会漏掉。
        切换翻译引擎会重建输入框，所以这个方法也要在重建之后再跑一次。
        """
        # PySide6 的 findChildren 一次只收一个类型，分开找
        for cls in (QComboBox, QAbstractSpinBox):
            for w in self.findChildren(cls):
                self._wheel_guard.guard(w)

        # 按钮改成「只能用 Tab 走到，点击不抢焦点」。
        #
        # 不改的话有一条挺阴的链子：点「测试连接」→ 按钮拿到焦点 → 代码把它
        # setEnabled(False) → Qt 发现当前焦点控件被禁用了，就把焦点推给 Tab 顺序里的
        # 下一个，正好是「中文译成」→ 它一有焦点，上面那个滚轮守卫就不拦了 →
        # 这时候滚一下页面，翻译目标语言就被悄悄改掉了。
        # 表面现象只是「点完测试，中文译成那一栏亮起来了」，实际后果比看起来严重。
        for btn in self.findChildren(QPushButton):
            btn.setFocusPolicy(Qt.FocusPolicy.TabFocus)

    # ------------------------------------------------------------ 布局工具
    def _card(self, title: str = "") -> QVBoxLayout:
        if title:
            label = QLabel(title)
            label.setObjectName("SectionTitle")
            self.column.addWidget(label)

        card = QFrame()
        card.setObjectName("Card")
        inner = QVBoxLayout(card)
        inner.setContentsMargins(15, 13, 15, 13)
        inner.setSpacing(10)
        self.column.addWidget(card)
        return inner

    @staticmethod
    def _row(label: str, widget: QWidget) -> QWidget:
        row = QWidget()
        lay = QHBoxLayout(row)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(10)
        text = QLabel(label)
        text.setFixedWidth(LABEL_W)
        text.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        lay.addWidget(text)
        lay.addWidget(widget, 1)
        return row

    @staticmethod
    def _hint(text: str) -> QLabel:
        label = QLabel(text)
        label.setObjectName("Hint")
        label.setWordWrap(True)
        return label

    @staticmethod
    def _keys(pairs: list[tuple[str, str]]) -> QWidget:
        """一张「按键 → 作用」的小表，用来把操作说明排整齐。"""
        host = QWidget()
        grid = QVBoxLayout(host)
        grid.setContentsMargins(0, 0, 0, 0)
        grid.setSpacing(5)
        for keys, what in pairs:
            line = QWidget()
            lay = QHBoxLayout(line)
            lay.setContentsMargins(0, 0, 0, 0)
            lay.setSpacing(10)
            k = QLabel(keys)
            k.setObjectName("Key")
            k.setFixedWidth(132)
            k.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            lay.addWidget(k)
            v = QLabel(what)
            v.setObjectName("Hint")
            v.setWordWrap(True)
            lay.addWidget(v, 1)
            grid.addWidget(line)
        return host

    # -------------------------------------------------------------- 快捷键
    def _build_hotkey(self):
        self._page("快捷键", "点一下输入框，然后直接按下想要的组合键。", "keyboard")

        card = self._card()
        card.setSpacing(16)          # 一个快捷键一组，组内挨紧、组间拉开
        for name, key, default, label, desc in HOTKEYS:
            group = QWidget()
            box = QVBoxLayout(group)
            box.setContentsMargins(0, 0, 0, 0)
            box.setSpacing(4)

            edit = HotkeyEdit(self.cfg.get(key, default))
            edit.setMinimumHeight(34)
            # 别让它跟着窗口一起拉宽：一个「Ctrl+Alt+Q」摊在 600px 的框里居中，
            # 看着像个输入框坏了。给一个够放最长组合键的固定宽度就行。
            edit.setMaximumWidth(240)
            font = QFont()
            font.setPixelSize(14)
            edit.setFont(font)
            edit.hotkeyChanged.connect(lambda spec, n=name: self._on_hotkey(n, spec))
            self.hotkey_edits[name] = edit
            box.addWidget(self._row(label, edit))

            note = self._hint(desc)
            note.setContentsMargins(LABEL_W + 10, 0, 0, 0)
            box.addWidget(note)

            status = QLabel("")
            status.setObjectName("Status")
            status.setWordWrap(True)
            status.setContentsMargins(LABEL_W + 10, 0, 0, 0)
            self.hotkey_status[name] = status
            box.addWidget(status)

            card.addWidget(group)

        tips = self._card("框选时")
        tips.addWidget(self._keys([
            ("拖动鼠标", "画出要翻译的区域，松手立刻识别并翻译"),
            ("Esc / 右键", "取消这次框选"),
        ]))
        self.column.addStretch(1)

    def _on_hotkey(self, name: str, spec: str):
        if parse_hotkey(spec) is None:
            self.hotkey_status[name].setText("这个组合不可用")
            return
        key = next(k for n, k, _d, _l, _s in HOTKEYS if n == name)
        self.cfg.set(key, spec)
        self._save()
        self._refresh_side_foot()
        self.hotkeyEdited.emit(name, spec)

    def set_hotkey_status(self, name: str, ok: bool, message: str):
        status = self.hotkey_status.get(name)
        if status is None:
            return
        status.setText(message if message else ("已生效" if ok else ""))
        status.setStyleSheet("color:#5FD08A;" if ok else "color:#FF6B6B;")

    # ------------------------------------------------------------ 翻译引擎
    def _build_translator(self):
        self._page("翻译", "识别到的文字送到哪个接口去翻。改完记得点一下「测试连接」。", "globe")

        card = self._card()
        self.provider_combo = QComboBox()
        for name, cls in translators.PROVIDERS.items():
            self.provider_combo.addItem(cls.label, name)
        current = self.cfg.get("translator.provider", "microsoft")
        idx = self.provider_combo.findData(current)
        self.provider_combo.setCurrentIndex(max(0, idx))
        self.provider_combo.currentIndexChanged.connect(self._on_provider)
        card.addWidget(self._row("接口", self.provider_combo))

        self.provider_hint = self._hint("")
        self.provider_hint.setContentsMargins(LABEL_W + 10, 0, 0, 0)
        card.addWidget(self.provider_hint)

        self.fields_host = QWidget()
        self.fields_layout = QVBoxLayout(self.fields_host)
        self.fields_layout.setContentsMargins(0, 0, 0, 0)
        self.fields_layout.setSpacing(9)
        card.addWidget(self.fields_host)

        bar = QHBoxLayout()
        bar.setSpacing(9)
        bar.addSpacing(LABEL_W + 10)
        self.test_btn = QPushButton("测试连接")
        self.test_btn.clicked.connect(self._on_test)
        bar.addWidget(self.test_btn)
        self.test_status = QLabel("")
        self.test_status.setObjectName("Status")
        self.test_status.setWordWrap(True)
        self.test_status.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Preferred)
        bar.addWidget(self.test_status, 1)
        card.addLayout(bar)

        lang = self._card("语言")
        self.zh_target = QComboBox()
        for code, name in _ZH_TARGETS:
            self.zh_target.addItem(name, code)
        idx = self.zh_target.findData(self.cfg.get("lang.zh_target", "en"))
        self.zh_target.setCurrentIndex(max(0, idx))
        self.zh_target.currentIndexChanged.connect(self._on_zh_target)
        lang.addWidget(self._row("中文译成", self.zh_target))
        lang.addWidget(self._hint("其余语言一律译成简体中文，程序自动判断，无需手动切换。"))

        self.column.addStretch(1)
        self._rebuild_fields()

    def _current_provider(self) -> str:
        return self.provider_combo.currentData()

    def _on_provider(self):
        # 先把界面切过去，再存盘。存盘是可能失败的（配置文件被占用等），
        # 放前面的话一旦抛异常，下面的字段就永远停在上一个接口上。
        self.cfg.set("translator.provider", self._current_provider())
        self.test_status.setText("")
        self._rebuild_fields()
        self._save()
        self.settingsSaved.emit()

    def _rebuild_fields(self):
        # 必须 setParent(None)，光 deleteLater() 是不够的：
        # takeAt() 只是把控件移出布局，它还挂在 fields_host 上、还带着旧的几何位置，
        # 而 deleteLater 要等事件循环空闲才真的删——这中间旧的一排输入框
        # 就那样**盖在**新的一排上面。换接口后还显示上一个接口的字段就是这么来的。
        while self.fields_layout.count():
            item = self.fields_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.setParent(None)
                widget.deleteLater()
        self._provider_fields.clear()

        provider = self._current_provider()
        self.provider_hint.setText(translators.HINTS.get(provider, ""))

        supports_models = getattr(translators.PROVIDERS[provider], "supports_model_list", False)

        for key, label, secret, placeholder in translators.FIELDS.get(provider, []):
            value = str(self.cfg.get(f"translator.{provider}.{key}", "") or "")
            if key == "model" and supports_models:
                self.fields_layout.addWidget(self._row(label, self._model_picker(provider, value, placeholder)))
                continue

            edit = QLineEdit(value)
            edit.setPlaceholderText(placeholder)
            if secret:
                edit.setEchoMode(QLineEdit.EchoMode.PasswordEchoOnEdit)
            edit.textEdited.connect(
                lambda text, p=provider, k=key: self._save_field(p, k, text)
            )
            self._provider_fields[key] = edit
            self.fields_layout.addWidget(self._row(label, edit))

        if hasattr(self, "_wheel_guard"):
            self._guard_wheel()

    def _model_picker(self, provider: str, value: str, placeholder: str) -> QWidget:
        """模型栏：可编辑下拉框 + 刷新按钮。

        很多接口（尤其是英伟达）填了不存在的模型名只会回 404，
        光看报错根本猜不到是模型的问题，所以直接把可用列表拉下来给人选。
        """
        host = QWidget()
        lay = QHBoxLayout(host)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(6)

        combo = QComboBox()
        combo.setEditable(True)
        combo.lineEdit().setPlaceholderText(placeholder)
        if value:
            combo.addItem(value)
            combo.setCurrentText(value)
        else:
            combo.setCurrentText("")
        combo.currentTextChanged.connect(
            lambda text, p=provider: self._save_field(p, "model", text)
        )
        self.model_combo = combo
        lay.addWidget(combo, 1)

        btn = QPushButton("刷新")
        btn.setFixedWidth(52)
        btn.setToolTip("拉取该账号当前可用的模型")
        btn.clicked.connect(lambda: self._fetch_models(provider))
        self.model_btn = btn
        lay.addWidget(btn)
        return host

    def _fetch_models(self, provider: str):
        if getattr(self, "_models_thread", None) and self._models_thread.isRunning():
            return
        self.test_status.setStyleSheet("color:#7E838C;")
        self.test_status.setText("正在拉取模型列表…")
        # 键盘可以用 Tab 聚焦这个按钮。直接禁用当前焦点控件时，Qt 会把焦点
        # 推给下一个输入框；先清掉，避免滚动页面时误改那个输入框的值。
        self.model_btn.clearFocus()
        self.model_btn.setEnabled(False)
        self._models_provider = provider
        self._models_thread = _ModelsThread(provider, self.cfg.get(f"translator.{provider}", {}) or {})
        self._models_thread.done.connect(self._on_models)
        self._models_thread.start()

    def _on_models(self, ok: bool, payload):
        requested_provider = self._models_provider
        self._models_provider = None
        if requested_provider != self._current_provider():
            return
        # 拉取过程中用户可能已经切走了引擎，那些控件已经被销毁
        try:
            self.model_btn.setEnabled(True)
        except RuntimeError:
            return
        if not ok:
            self.test_status.setStyleSheet("color:#FF6B6B;")
            options = self.cfg.get(f"translator.{requested_provider}", {}) or {}
            message = redact_sensitive(payload, configured_secrets(options))
            self.test_status.setText(message[:160])
            return

        keep = self.model_combo.currentText().strip()
        self.model_combo.blockSignals(True)
        self.model_combo.clear()
        self.model_combo.addItems(payload)
        # 原来填的那个还在列表里就保留，不在就选第一个可用的
        self.model_combo.setCurrentText(keep if keep in payload else payload[0])
        self.model_combo.blockSignals(False)
        self._save_field(self._current_provider(), "model", self.model_combo.currentText())

        self.test_status.setStyleSheet("color:#5FD08A;")
        self.test_status.setText(
            f"拉到 {len(payload)} 个模型" + ("" if keep in payload else f"，已选 {self.model_combo.currentText()}")
        )
        self._guard_wheel()

    def _save_field(self, provider: str, key: str, text: str):
        self.cfg.set(f"translator.{provider}.{key}", text.strip())
        self._save()
        self.settingsSaved.emit()

    def _on_test(self):
        if self._test_thread and self._test_thread.isRunning():
            return
        provider = self._current_provider()
        self.test_status.setStyleSheet("color:#7E838C;")
        self.test_status.setText("测试中…")
        # 鼠标点击不会让 TabFocus 按钮拿焦点，但键盘 Space 会。无论从哪条
        # 路径进来，都先清焦点再禁用，不能把焦点自动推到「中文译成」。
        self.test_btn.clearFocus()
        self.test_btn.setEnabled(False)
        self._test_provider = provider
        self._test_thread = _TestThread(provider, self.cfg.get(f"translator.{provider}", {}) or {})
        self._test_thread.done.connect(self._on_test_done)
        self._test_thread.start()

    def _on_test_done(self, ok: bool, message: str):
        requested_provider = self._test_provider
        self._test_provider = None
        self.test_btn.setEnabled(True)
        if requested_provider != self._current_provider():
            self.test_status.clear()
            return
        options = self.cfg.get(f"translator.{requested_provider}", {}) or {}
        message = redact_sensitive(message, configured_secrets(options))
        self.test_status.setStyleSheet("color:#5FD08A;" if ok else "color:#FF6B6B;")
        self.test_status.setText(message if len(message) < 160 else message[:157] + "…")

    def _on_zh_target(self):
        self.cfg.set("lang.zh_target", self.zh_target.currentData())
        self._save()
        self.settingsSaved.emit()

    # ------------------------------------------------------------ 文字识别
    def _build_ocr(self):
        self._page("文字识别", "先把屏幕上的字读出来，才谈得上翻译。", "scan")

        card = self._card()
        engines = available_engines()
        self.ocr_engine = QComboBox()
        for name, label in engines.items():
            self.ocr_engine.addItem(label, name)
        if not engines:
            self.ocr_engine.addItem("没有可用的 OCR 引擎", "windows")
        idx = self.ocr_engine.findData(self.cfg.get("ocr.engine", "windows"))
        self.ocr_engine.setCurrentIndex(max(0, idx))
        self.ocr_engine.currentIndexChanged.connect(self._on_ocr_engine)
        card.addWidget(self._row("引擎", self.ocr_engine))

        self.ocr_cloud_note = self._hint(
            ""
        )
        self.ocr_cloud_note.setContentsMargins(LABEL_W + 10, 0, 0, 0)
        card.addWidget(self.ocr_cloud_note)

        self.azure_ocr_fields = QWidget()
        azure_fields_layout = QVBoxLayout(self.azure_ocr_fields)
        azure_fields_layout.setContentsMargins(0, 0, 0, 0)
        azure_fields_layout.setSpacing(9)
        azure_opts = self.cfg.get("ocr.azure_vision", {}) or {}
        self.azure_ocr_endpoint = QLineEdit(str(azure_opts.get("endpoint") or ""))
        self.azure_ocr_endpoint.setPlaceholderText(
            "https://资源名.cognitiveservices.azure.com/"
        )
        self.azure_ocr_endpoint.editingFinished.connect(self._save_azure_ocr)
        azure_fields_layout.addWidget(self._row("Endpoint", self.azure_ocr_endpoint))
        self.azure_ocr_key = QLineEdit(str(azure_opts.get("key") or ""))
        self.azure_ocr_key.setEchoMode(QLineEdit.EchoMode.Password)
        self.azure_ocr_key.setPlaceholderText("Azure Vision 的 KEY 1 或 KEY 2")
        self.azure_ocr_key.editingFinished.connect(self._save_azure_ocr)
        azure_fields_layout.addWidget(self._row("Key", self.azure_ocr_key))
        card.addWidget(self.azure_ocr_fields)

        # 切引擎必须重启，那就把「立刻重启」直接放在旁边，别让人去找
        self.ocr_restart_row = QWidget()
        row = QHBoxLayout(self.ocr_restart_row)
        row.setContentsMargins(LABEL_W + 10, 0, 0, 0)
        row.setSpacing(9)
        self.ocr_restart_hint = QLabel("换了引擎，重启后才会生效。")
        self.ocr_restart_hint.setObjectName("Hint")
        self.ocr_restart_hint.setStyleSheet("color:#E8B44A;")
        self.ocr_restart_hint.setWordWrap(True)
        row.addWidget(self.ocr_restart_hint, 1)
        now_btn = QPushButton("立刻重启")
        now_btn.setObjectName("Primary")
        now_btn.clicked.connect(self.restartRequested)
        row.addWidget(now_btn)
        self.ocr_restart_row.setVisible(False)
        card.addWidget(self.ocr_restart_row)

        installed = windows_ocr.available_languages()
        cjk = [t for t in installed if t.lower().startswith(("zh", "ja", "ko"))]
        latin = [t for t in installed if t not in cjk]

        self.ocr_lang = QComboBox()
        if installed:
            self.ocr_lang.addItem("自动（推荐）", cjk + latin)
            for tag in installed:
                self.ocr_lang.addItem(f"只用{_lang_name(tag)}", [tag])
        saved = self.cfg.get("ocr.languages") or []
        match = next(
            (i for i in range(self.ocr_lang.count()) if self.ocr_lang.itemData(i) == saved), 0
        )
        self.ocr_lang.setCurrentIndex(match)
        self.ocr_lang.currentIndexChanged.connect(self._on_ocr_lang)
        self.ocr_lang.setEnabled(self.ocr_engine.currentData() == "windows")
        card.addWidget(self._row("识别语言", self.ocr_lang))

        self._refresh_ocr_engine_ui(self.ocr_engine.currentData())

        names = "、".join(_lang_name(t) for t in installed) or "无"
        lang_note = self._hint(f"系统已安装：{names}")
        lang_note.setContentsMargins(LABEL_W + 10, 0, 0, 0)
        card.addWidget(lang_note)

        if "rapidocr" not in engines:
            import sys as _sys

            tip = self._card("识别不出艺术字？")
            tip.addWidget(
                self._hint(
                    "系统 OCR 是照着文档和界面文字训练的，视频封面、海报那种花体字/描边字"
                    "基本读不出来。RapidOCR 明显更准（离线），代价是慢十几倍、体积大 200MB。"
                )
            )
            # 打包成 exe 之后 pip 是没用的，得重新打一个带 RapidOCR 的版本
            frozen = getattr(_sys, "frozen", False)
            code = QLabel("python build.py --with-rapidocr" if frozen
                          else "pip install rapidocr-onnxruntime")
            code.setObjectName("Code")
            code.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
            tip.addWidget(code)
            tip.addWidget(self._hint(
                "当前这个版本没有打包 RapidOCR。需要的话用上面的命令重新打包一份。"
                if frozen else "装完重启本程序，上面的「引擎」里就会多出一个选项。"))

        more = self._card("装其他语言")
        more.addWidget(
            self._hint(
                "需要日语、韩语等语言时，到「Windows 设置 → 时间和语言 → 语言和区域」"
                "添加该语言，再进入其「语言选项 → 可选功能」安装「光学字符识别」。"
                "装好后重启本程序即可。"
            )
        )
        self.column.addStretch(1)

    def _on_ocr_engine(self):
        engine = self.ocr_engine.currentData()
        self.cfg.set("ocr.engine", engine)
        self._save()
        self._refresh_ocr_engine_ui(engine)
        self.settingsSaved.emit()

    def _refresh_ocr_engine_ui(self, engine: str):
        # RapidOCR 依赖的 onnxruntime 必须在 Qt 之前加载，只能靠重启
        self.ocr_restart_row.setVisible(engine == "rapidocr")
        notes = {
            "azure_vision": (
                "框选截图会上传到你的 Azure Vision 资源。官方接口，免费层每月 5,000 次。"
            ),
            "youdao_cloud": (
                "框选截图会上传到有道服务器。无需密钥，但属于非官方接口，可能限流或改版失效。"
            ),
        }
        self.ocr_cloud_note.setText(notes.get(engine, ""))
        self.ocr_cloud_note.setVisible(engine in notes)
        self.azure_ocr_fields.setVisible(engine == "azure_vision")
        self.ocr_lang.setEnabled(engine == "windows")

    def _save_azure_ocr(self):
        self.cfg.set("ocr.azure_vision.endpoint", self.azure_ocr_endpoint.text().strip())
        self.cfg.set("ocr.azure_vision.key", self.azure_ocr_key.text().strip())
        self._save()
        self.settingsSaved.emit()

    def _on_ocr_lang(self):
        data = self.ocr_lang.currentData()
        if data:
            self.cfg.set("ocr.languages", list(data))
            self._save()
            self.settingsSaved.emit()

    # ---------------------------------------------------------------- 显示
    def _build_appearance(self):
        self._page("显示", "译文覆盖在原文上之后的样子和行为。", "contrast")

        card = self._card()
        self.font_combo = QFontComboBox()
        # 两道限制，缺一不可：
        # 1) 关掉可编辑。QFontComboBox 默认能输入，框里有焦点时敲个字母会被**追加**进
        #    字体名，「Microsoft YaHei UI」+ s = 「Microsoft YaHei UIs」，这名字不存在。
        # 2) 只列**能显示简体中文**的字体（325 个筛到 52 个）。
        #    光关掉可编辑还不够：非编辑态下敲字母会跳到该字母开头的字体上，
        #    一不小心就跳到 Franklin Gothic 这种没有汉字的字体，
        #    译文里的中文只能靠系统替换，出来又丑又不统一。
        #    能显示中文的字体必然也能显示拉丁字母，所以这么筛不会有损失。
        self.font_combo.setEditable(False)
        self.font_combo.setWritingSystem(QFontDatabase.WritingSystem.SimplifiedChinese)
        self.font_combo.setCurrentFont(QFont(self.cfg.get("appearance.font_family", "Microsoft YaHei UI")))
        self.font_combo.currentFontChanged.connect(self._on_font)
        card.addWidget(self._row("译文字体", self.font_combo))
        note = self._hint("只列出能显示中文的字体——没有汉字的字体会让译文退化成系统替换字，很难看。")
        note.setContentsMargins(LABEL_W + 10, 0, 0, 0)
        card.addWidget(note)

        self.accent_row = SwatchRow(self.accent)
        self.accent_row.picked.connect(self._on_accent)
        card.addWidget(self._row("强调色", self.accent_row))
        self.accent_hint = self._hint(
            "框选边框、八个拖拽手柄、进度条和菜单高亮都用这个颜色。"
            "改完立刻生效；已经开着的译文窗口保持原色，下一个才换。"
        )
        self.accent_hint.setContentsMargins(LABEL_W + 10, 0, 0, 0)
        card.addWidget(self.accent_hint)

        self.close_mode = QComboBox()
        for code, name in _CLOSE_MODES:
            self.close_mode.addItem(name, code)
        idx = self.close_mode.findData(self.cfg.get("appearance.close_mode", "click"))
        self.close_mode.setCurrentIndex(max(0, idx))
        self.close_mode.currentIndexChanged.connect(self._on_close_mode)
        card.addWidget(self._row("关闭方式", self.close_mode))

        self.timeout_spin = QSpinBox()
        self.timeout_spin.setRange(1, 60)
        self.timeout_spin.setSuffix(" 秒")
        self.timeout_spin.setValue(int(self.cfg.get("appearance.timeout_ms", 5000)) // 1000)
        self.timeout_spin.valueChanged.connect(self._on_timeout)
        self.timeout_row = self._row("停留时长", self.timeout_spin)
        card.addWidget(self.timeout_row)
        self.timeout_row.setVisible(self.close_mode.currentData() == "timeout")

        self.auto_copy = QCheckBox("译文自动复制到剪贴板")
        self.auto_copy.setChecked(bool(self.cfg.get("appearance.auto_copy", True)))
        self.auto_copy.toggled.connect(self._on_auto_copy)
        card.addWidget(self.auto_copy)

        ops = self._card("译文窗口怎么用")
        toggle_key = self.cfg.get("hotkey_toggle", "Ctrl+Alt+W")
        ops.addWidget(self._keys([
            ("右下角 —", f"收到托盘，按 {toggle_key} 或托盘菜单叫回来"),
            ("右下角 ✕ / Esc", "关闭"),
            ("M", "收到托盘"),
            ("拖动那根控制条", "移动窗口（条上那几个点就是拖拽手柄）"),
            ("方向键", "微调位置，按住 Shift 步子更大"),
            ("拖动边框 / 四个角", "改成框住屏幕上的另一块，松手就重新识别并翻译；"
                                  "画面不会被拉伸变形"),
            ("拖动时按住 Shift", "锁住长宽比"),
            ("双击空白处 / Home", "回到刚框选时的范围（也会重新翻一次）"),
            ("在译文上拖动", "划选文字；Ctrl+C 复制选中，Ctrl+A 复制全部"),
            ("点画面空白处", "取消当前划选"),
            ("按住空格 / 右键", "临时看回原文"),
        ]))
        self.column.addStretch(1)

    def _on_font(self, font: QFont):
        family = font.family()
        # 再兜一道：系统里没有、或者显示不了中文的字体都别存。
        # 手改过 config.json、以后哪个 Qt 版本又让下拉框能输入了，都能挡住。
        from ..render import covers_chinese

        if family not in QFontDatabase.families() or not covers_chinese(family):
            print(f"[settings] {family!r} 不存在或显示不了中文，不保存")
            return
        self.cfg.set("appearance.font_family", family)
        self._save()
        self.settingsSaved.emit()

    def _on_accent(self, color: str):
        self.accent = color
        self.cfg.set("appearance.accent", color)
        self._save()
        # 设置窗口自己立刻换色，看得见效果；但托盘图标、已经开着的译文窗口
        # 都是启动时按旧颜色建好的，那些得重启。这一点在旁边的说明里写清楚了。
        self.setStyleSheet(build_qss(color))
        self.settingsSaved.emit()

    def _on_close_mode(self):
        mode = self.close_mode.currentData()
        self.cfg.set("appearance.close_mode", mode)
        self._save()
        self.timeout_row.setVisible(mode == "timeout")
        self.settingsSaved.emit()

    def _on_timeout(self, seconds: int):
        self.cfg.set("appearance.timeout_ms", seconds * 1000)
        self._save()
        self.settingsSaved.emit()

    def _on_auto_copy(self, checked: bool):
        self.cfg.set("appearance.auto_copy", checked)
        self._save()
        self.settingsSaved.emit()

    # ---------------------------------------------------------------- 其他
    def _build_misc(self):
        from .. import winsys

        self._page("其他", "开机自启、配置文件、出问题时怎么查。", "sliders")

        card = self._card()
        self.autostart = QCheckBox("开机自动启动")
        self.autostart.setChecked(winsys.get_autostart())
        self.autostart.toggled.connect(self._on_autostart)
        card.addWidget(self.autostart)

        card.addWidget(self._hint("装完之后每次开机自动到托盘待命，不会弹窗口。"))

        sep = QFrame()
        sep.setObjectName("Sep")
        sep.setFixedHeight(1)      # 样式表里的 max-height 只封顶，不给高度，不写这行它是 0 高
        card.addWidget(sep)

        restart_row = QHBoxLayout()
        restart_btn = QPushButton("重启程序")
        restart_btn.clicked.connect(self.restartRequested)
        restart_row.addWidget(restart_btn)
        restart_row.addStretch(1)
        card.addLayout(restart_row)
        card.addWidget(self._hint("换了 OCR 引擎、或者刚给 Windows 装完新的识别语言包，重启一下才会生效。"))

        cfg_card = self._card("配置文件")
        path_label = self._hint(str(CONFIG_PATH))
        path_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        cfg_card.addWidget(path_label)
        open_btn = QPushButton("在资源管理器中打开")
        open_btn.clicked.connect(self._open_config_dir)
        row = QHBoxLayout()
        row.addWidget(open_btn)
        row.addStretch(1)
        cfg_card.addLayout(row)
        cfg_card.addWidget(
            self._hint(
                "这里改的每一项都即时写盘。API 密钥用 Windows 的 DPAPI 加过密，"
                "只有当前这台电脑上的当前这个账户能解开——但配置文件本身仍然别随手发给别人。"
            )
        )

        diag = self._card("出问题时")
        diag.addWidget(
            self._hint(
                "命令行跑一次自检，会逐项检查 OCR 语言包 → 文字识别 → 译文排版 → "
                "翻译接口 → 全局快捷键，一眼看出断在哪一环："
            )
        )
        code = QLabel("ScreenTranslate.exe --selftest")
        code.setObjectName("Code")
        code.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        diag.addWidget(code)
        self.column.addStretch(1)

    def _on_autostart(self, checked: bool):
        from .. import winsys

        try:
            winsys.set_autostart(checked)
            self.cfg.set("autostart", checked)
            self._save()
        except Exception as exc:
            self.autostart.blockSignals(True)
            self.autostart.setChecked(not checked)
            self.autostart.blockSignals(False)
            print(f"[settings] 设置开机自启失败：{exc}")

    def _open_config_dir(self):
        CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
        if not CONFIG_PATH.exists():
            self._save()
        subprocess.Popen(["explorer", "/select,", str(CONFIG_PATH)])

    # -------------------------------------------------------------- 生命周期
    def shutdown(self) -> None:
        """Cancel settings requests and give both a fixed total cleanup window."""
        threads = [self._test_thread, getattr(self, "_models_thread", None)]
        for thread in threads:
            if thread is not None and thread.isRunning():
                thread.cancel()
        deadline = time.monotonic() + SHUTDOWN_WAIT_MS / 1000
        for thread in threads:
            if thread is not None and thread.isRunning():
                remaining = max(0, int((deadline - time.monotonic()) * 1000))
                thread.wait(remaining)

    def closeEvent(self, event):
        self._save()
        super().closeEvent(event)

    def show_front(self):
        self.show()
        self.setWindowState(
            (self.windowState() & ~Qt.WindowState.WindowMinimized) | Qt.WindowState.WindowActive
        )
        self.raise_()
        self.activateWindow()
