"""快捷键录制框：点一下，按下想要的组合键即可。"""
from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QKeySequence
from PySide6.QtWidgets import QLineEdit

from ..hotkey import normalize, parse

_MOD_KEYS = {
    Qt.Key.Key_Control,
    Qt.Key.Key_Shift,
    Qt.Key.Key_Alt,
    Qt.Key.Key_Meta,
    Qt.Key.Key_AltGr,
}


class HotkeyEdit(QLineEdit):
    hotkeyChanged = Signal(str)

    def __init__(self, value: str = "", parent=None):
        super().__init__(parent)
        self.setReadOnly(True)
        self.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.setPlaceholderText("点这里，然后按下组合键")
        self._value = value
        self.setText(value)

    def value(self) -> str:
        return self._value

    def focusInEvent(self, event):
        super().focusInEvent(event)
        self.setText("按下组合键…")

    def focusOutEvent(self, event):
        super().focusOutEvent(event)
        self.setText(self._value)

    def keyPressEvent(self, event):
        key = event.key()
        if key in _MOD_KEYS:
            return
        if key == Qt.Key.Key_Escape:
            self.setText(self._value)
            self.clearFocus()
            return

        mods = event.modifiers()
        parts = []
        if mods & Qt.KeyboardModifier.ControlModifier:
            parts.append("Ctrl")
        if mods & Qt.KeyboardModifier.AltModifier:
            parts.append("Alt")
        if mods & Qt.KeyboardModifier.ShiftModifier:
            parts.append("Shift")
        if mods & Qt.KeyboardModifier.MetaModifier:
            parts.append("Win")

        name = QKeySequence(key).toString()
        if not name:
            return
        parts.append(name)
        spec = normalize("+".join(parts))

        if parse(spec) is None:
            self.setText("这个组合不能用（除 F1~F12 外必须带 Ctrl/Alt/Shift/Win）")
            return

        self._value = spec
        self.setText(spec)
        self.hotkeyChanged.emit(spec)
        self.clearFocus()
