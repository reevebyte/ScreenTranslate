"""全局快捷键。用 Win32 RegisterHotKey，不需要管理员权限，也不会像键盘钩子那样被杀软误报。"""
from __future__ import annotations

import ctypes
import ctypes.wintypes

from PySide6.QtCore import Signal
from PySide6.QtWidgets import QWidget

user32 = ctypes.windll.user32

MOD_ALT = 0x0001
MOD_CONTROL = 0x0002
MOD_SHIFT = 0x0004
MOD_WIN = 0x0008
MOD_NOREPEAT = 0x4000

WM_HOTKEY = 0x0312

_MODS = {
    "ctrl": MOD_CONTROL, "control": MOD_CONTROL,
    "alt": MOD_ALT,
    "shift": MOD_SHIFT,
    "win": MOD_WIN, "meta": MOD_WIN, "super": MOD_WIN,
}

_NAMED_VK = {
    "space": 0x20, "tab": 0x09, "enter": 0x0D, "return": 0x0D, "esc": 0x1B, "escape": 0x1B,
    "backspace": 0x08, "insert": 0x2D, "delete": 0x2E, "del": 0x2E,
    "home": 0x24, "end": 0x23, "pageup": 0x21, "pagedown": 0x22,
    "left": 0x25, "up": 0x26, "right": 0x27, "down": 0x28,
    "`": 0xC0, "-": 0xBD, "=": 0xBB, "[": 0xDB, "]": 0xDD, "\\": 0xDC,
    ";": 0xBA, "'": 0xDE, ",": 0xBC, ".": 0xBE, "/": 0xBF,
    "printscreen": 0x2C, "pause": 0x13,
}


def parse(spec: str) -> tuple[int, int] | None:
    """'Ctrl+Alt+Q' -> (修饰键掩码, 虚拟键码)。解析不了返回 None。"""
    if not spec:
        return None
    parts = [p.strip().lower() for p in spec.replace("＋", "+").split("+") if p.strip()]
    if not parts:
        return None
    mods = 0
    key = None
    for p in parts:
        if p in _MODS:
            mods |= _MODS[p]
        else:
            key = p
    if key is None:
        return None

    if len(key) == 1 and key.isalnum():
        vk = ord(key.upper())
    elif key.startswith("f") and key[1:].isdigit() and 1 <= int(key[1:]) <= 24:
        vk = 0x70 + int(key[1:]) - 1
    elif key in _NAMED_VK:
        vk = _NAMED_VK[key]
    else:
        return None

    # 不带任何修饰键时只允许 F1~F24，否则会把整个系统的这个键都吃掉
    if mods == 0 and not (0x70 <= vk <= 0x87):
        return None
    return mods, vk


def normalize(spec: str) -> str:
    parts = [p.strip() for p in spec.replace("＋", "+").split("+") if p.strip()]
    order = {"ctrl": 0, "control": 0, "alt": 1, "shift": 2, "win": 3, "meta": 3, "super": 3}
    mods = sorted([p for p in parts if p.lower() in order], key=lambda p: order[p.lower()])
    keys = [p for p in parts if p.lower() not in order]
    canon = {"control": "Ctrl", "meta": "Win", "super": "Win"}
    out = [canon.get(m.lower(), m.capitalize()) for m in mods]
    out += [k.upper() if len(k) == 1 else k.capitalize() for k in keys]
    return "+".join(out)


class HotkeyHost(QWidget):
    """一个不可见的窗口，专门用来接收 WM_HOTKEY 消息。

    可以同时挂多个快捷键，用名字区分（"capture" 框选、"toggle" 缩小/显示）。
    触发时把名字发出来，由上层决定做什么。
    """

    triggered = Signal(str)

    BASE_ID = 0xA731

    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("ScreenTranslate.HotkeyHost")
        self.resize(0, 0)
        self._ids: dict[str, int] = {}       # 名字 -> Win32 hotkey id（分配后不再变）
        self._live: dict[int, str] = {}      # 当前真正注册着的 id -> 名字
        self._specs: dict[str, str] = {}     # 名字 -> 组合键
        self._hwnd = int(self.winId())       # 强制创建原生窗口句柄

    def _id_for(self, name: str) -> int:
        if name not in self._ids:
            self._ids[name] = self.BASE_ID + len(self._ids)
        return self._ids[name]

    def register(self, name: str, spec: str) -> tuple[bool, str]:
        self.unregister(name)
        parsed = parse(spec)
        if parsed is None:
            return False, f"快捷键格式无法识别：{spec}"

        # 两个功能抢同一个组合时，Windows 只会回一句「已被占用」，
        # 看上去像是别的程序占了，很难想到是自己跟自己撞了，所以先自己查一遍。
        canon = normalize(spec).lower()
        for other, other_spec in self._specs.items():
            if other != name and normalize(other_spec).lower() == canon:
                return False, "这个组合已经分配给另一个功能了"

        mods, vk = parsed
        if not user32.RegisterHotKey(self._hwnd, self._id_for(name), mods | MOD_NOREPEAT, vk):
            err = ctypes.GetLastError()
            if err == 1409:  # ERROR_HOTKEY_ALREADY_REGISTERED
                return False, f"{spec} 已被其他程序占用，请换一个"
            return False, f"注册快捷键失败（错误码 {err}）"

        self._live[self._id_for(name)] = name
        self._specs[name] = spec
        return True, ""

    def unregister(self, name: str | None = None) -> None:
        """撤掉某一个快捷键；不给名字就全部撤掉。"""
        for hid, nm in list(self._live.items()):
            if name is None or nm == name:
                user32.UnregisterHotKey(self._hwnd, hid)
                self._live.pop(hid, None)
                self._specs.pop(nm, None)

    def nativeEvent(self, event_type, message):
        if bytes(event_type) == b"windows_generic_MSG":
            try:
                msg = ctypes.wintypes.MSG.from_address(_addr(message))
            except Exception:
                return False, 0
            if msg.message == WM_HOTKEY and msg.wParam in self._live:
                self.triggered.emit(self._live[msg.wParam])
                return True, 0
        return False, 0


def _addr(message) -> int:
    """PySide6 各版本给 nativeEvent 传的可能是 int，也可能是 voidptr。"""
    if isinstance(message, int):
        return message
    try:
        return int(message)
    except (TypeError, ValueError):
        return ctypes.cast(message, ctypes.c_void_p).value or 0
