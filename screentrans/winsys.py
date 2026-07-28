"""Win32 相关：DPI 感知、物理显示器矩形、开机自启。

坐标系说明（这是整个程序最容易出错的地方）：
  * Qt 用的是「逻辑坐标」，受系统缩放（125%/150%）影响。
  * 截屏和 OCR 用的是「物理像素」。
  * 两者的换算比例是 QScreen.devicePixelRatio()。
本模块负责把 QScreen 和它对应的物理矩形对上号。
"""
from __future__ import annotations

import ctypes
import sys
from ctypes import wintypes

user32 = ctypes.windll.user32


def enable_dpi_awareness() -> None:
    """必须在创建 QApplication 之前调用，否则多屏不同缩放时截屏会错位。"""
    if sys.platform != "win32":
        return
    # DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
    try:
        user32.SetProcessDpiAwarenessContext.argtypes = [ctypes.c_void_p]
        if user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4)):
            return
    except Exception:
        pass
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)  # PER_MONITOR_DPI_AWARE
        return
    except Exception:
        pass
    try:
        user32.SetProcessDPIAware()
    except Exception:
        pass


class _MONITORINFOEXW(ctypes.Structure):
    _fields_ = [
        ("cbSize", wintypes.DWORD),
        ("rcMonitor", wintypes.RECT),
        ("rcWork", wintypes.RECT),
        ("dwFlags", wintypes.DWORD),
        ("szDevice", wintypes.WCHAR * 32),
    ]


_MONITORENUMPROC = ctypes.WINFUNCTYPE(
    wintypes.BOOL,
    wintypes.HANDLE,
    wintypes.HDC,
    ctypes.POINTER(wintypes.RECT),
    wintypes.LPARAM,
)


def physical_monitor_rects() -> dict[str, tuple[int, int, int, int]]:
    """返回 {设备名(\\\\.\\DISPLAY1): (left, top, width, height)}，单位为物理像素。"""
    found: dict[str, tuple[int, int, int, int]] = {}

    def _cb(hmon, hdc, lprc, lparam):
        info = _MONITORINFOEXW()
        info.cbSize = ctypes.sizeof(_MONITORINFOEXW)
        if user32.GetMonitorInfoW(hmon, ctypes.byref(info)):
            r = info.rcMonitor
            found[info.szDevice] = (r.left, r.top, r.right - r.left, r.bottom - r.top)
        return True

    user32.EnumDisplayMonitors(0, None, _MONITORENUMPROC(_cb), 0)
    return found


def screen_physical_rect(screen) -> tuple[int, int, int, int]:
    """QScreen -> 该显示器的物理矩形。"""
    rects = physical_monitor_rects()
    name = screen.name()
    if name in rects:
        return rects[name]
    # 名字对不上时（少见）按逻辑几何 × dpr 推算
    geo = screen.geometry()
    dpr = screen.devicePixelRatio()
    return (
        int(round(geo.left() * dpr)),
        int(round(geo.top() * dpr)),
        int(round(geo.width() * dpr)),
        int(round(geo.height() * dpr)),
    )


# ------------------------------------------------------------ 窗口扩展样式

GWL_EXSTYLE = -20
WS_EX_NOACTIVATE = 0x08000000

# 64 位下要用 Ptr 版本，32 位上没有这个符号，退回普通版
_get_long = getattr(user32, "GetWindowLongPtrW", None) or user32.GetWindowLongW
_set_long = getattr(user32, "SetWindowLongPtrW", None) or user32.SetWindowLongW
_get_long.restype = ctypes.c_ssize_t
_get_long.argtypes = [ctypes.c_void_p, ctypes.c_int]
_set_long.restype = ctypes.c_ssize_t
_set_long.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_ssize_t]


def deny_activation(hwnd: int) -> bool:
    """给窗口加上 WS_EX_NOACTIVATE：点它不会把焦点抢过来。

    Qt 的 Qt.WindowDoesNotAcceptFocus 在这条路径上并没有落成这一位
    （实测扩展样式里只有 LAYERED | TOOLWINDOW | TOPMOST），只能自己补。
    没有它的话，点一下控制条上的按钮焦点就跑过去了，译文窗口再也收不到 Esc。
    """
    try:
        ex = _get_long(ctypes.c_void_p(hwnd), GWL_EXSTYLE)
        if ex & WS_EX_NOACTIVATE:
            return True
        _set_long(ctypes.c_void_p(hwnd), GWL_EXSTYLE, ex | WS_EX_NOACTIVATE)
        return bool(_get_long(ctypes.c_void_p(hwnd), GWL_EXSTYLE) & WS_EX_NOACTIVATE)
    except Exception:
        return False


# ---------------------------------------------------------------- 开机自启

_RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"


def launch_argv() -> list[str]:
    """重新启动自己要用的命令行。打包成 exe 和从源码跑是两回事。"""
    import os

    if getattr(sys, "frozen", False):
        return [sys.executable]
    pyw = sys.executable
    if pyw.lower().endswith("python.exe"):
        cand = pyw[:-10] + "pythonw.exe"      # 用 pythonw 才不会弹控制台黑框
        if os.path.exists(cand):
            pyw = cand
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    return [pyw, os.path.join(root, "run.pyw")]


def _launch_command() -> str:
    return " ".join(f'"{a}"' for a in launch_argv())


def relaunch(extra_args: list[str] | None = None) -> None:
    """启动一个新的自己，然后就不管了——调用方负责紧接着退出当前进程。

    DETACHED_PROCESS 是必须的：不脱离的话新进程会继承旧进程的控制台和作业对象，
    旧进程一退，新进程可能被一起收走。
    """
    import subprocess

    DETACHED_PROCESS = 0x00000008
    CREATE_NEW_PROCESS_GROUP = 0x00000200
    subprocess.Popen(
        launch_argv() + (extra_args or []),
        close_fds=True,
        creationflags=DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
    )


def set_autostart(enabled: bool) -> None:
    import winreg

    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, _RUN_KEY, 0, winreg.KEY_SET_VALUE) as k:
        if enabled:
            winreg.SetValueEx(k, "ScreenTranslate", 0, winreg.REG_SZ, _launch_command())
        else:
            try:
                winreg.DeleteValue(k, "ScreenTranslate")
            except FileNotFoundError:
                pass


def _registered_autostart_command() -> str | None:
    """读取当前账户的自启命令；不存在或类型异常时返回 ``None``。"""
    import winreg

    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, _RUN_KEY) as k:
            value, value_type = winreg.QueryValueEx(k, "ScreenTranslate")
    except OSError:
        return None
    if value_type not in {winreg.REG_SZ, winreg.REG_EXPAND_SZ} or not isinstance(value, str):
        return None
    return value


def get_autostart() -> bool:
    """只有注册表确实指向当前这份、仍存在的程序时才算已启用。"""
    import os

    command = _registered_autostart_command()
    argv = launch_argv()
    if command is None or command.strip().casefold() != _launch_command().casefold():
        return False
    return bool(argv) and all(os.path.isfile(path) for path in argv)


def ensure_autostart(enabled: bool) -> bool:
    """启用状态下修复升级或移动目录造成的旧注册表路径。

    返回 ``True`` 表示本次确实重写了注册表。禁用状态不主动碰注册表，避免
    覆盖用户用其他工具管理的启动项。
    """
    if not enabled or get_autostart():
        return False
    set_autostart(True)
    return True
