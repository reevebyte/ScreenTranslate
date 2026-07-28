"""用 Windows DPAPI 加密配置文件里的密钥。

DPAPI 的密钥由当前 Windows 账户派生，别的账户、别的机器都解不开，
不需要我们自己保管任何主密码。这不是「军用级加密」——同一个账户下运行的
任何程序都能解，但它挡住了最现实的那种泄漏：把 config.json 发给别人、
连着项目一起传到网盘、截图里带出去。

加密失败时必须让配置保存失败，绝不能悄悄把密钥明文落盘。
"""
from __future__ import annotations

import base64
import ctypes
import ctypes.wintypes
import sys

PREFIX = "dpapi:v1:"          # 认出「这串是加密过的」

_DESC = "ScreenTranslate API key"
CRYPTPROTECT_UI_FORBIDDEN = 0x1


class EncryptionError(RuntimeError):
    """DPAPI 无法保护密钥时阻止配置写盘。"""


class _BLOB(ctypes.Structure):
    _fields_ = [("cbData", ctypes.wintypes.DWORD),
                ("pbData", ctypes.POINTER(ctypes.c_char))]

    @classmethod
    def of(cls, data: bytes) -> "_BLOB":
        buf = ctypes.create_string_buffer(data, len(data))
        return cls(len(data), ctypes.cast(buf, ctypes.POINTER(ctypes.c_char)))

    def take(self) -> bytes:
        out = ctypes.string_at(self.pbData, self.cbData)
        ctypes.windll.kernel32.LocalFree(self.pbData)
        return out


def available() -> bool:
    return sys.platform == "win32"


def encrypt(text: str) -> str:
    """明文 -> 'dpapi:v1:<base64>'；失败时拒绝返回明文。"""
    if not text or text.startswith(PREFIX):
        return text
    if not available():
        raise EncryptionError("当前系统不支持 Windows DPAPI，密钥未保存")
    try:
        out = _BLOB()
        ok = ctypes.windll.crypt32.CryptProtectData(
            ctypes.byref(_BLOB.of(text.encode("utf-8"))), _DESC,
            None, None, None, CRYPTPROTECT_UI_FORBIDDEN, ctypes.byref(out),
        )
        if not ok:
            raise EncryptionError("Windows DPAPI 加密失败，密钥未保存")
        return PREFIX + base64.b64encode(out.take()).decode("ascii")
    except EncryptionError:
        raise
    except Exception as exc:
        raise EncryptionError(f"Windows DPAPI 加密失败，密钥未保存（{type(exc).__name__}）") from exc


def decrypt(text: str) -> str:
    """'dpapi:v1:<base64>' -> 明文。不是这个格式就原样返回（兼容老的明文配置）。"""
    if not isinstance(text, str) or not text.startswith(PREFIX):
        return text
    if not available():
        return ""
    try:
        blob = base64.b64decode(text[len(PREFIX):])
        out = _BLOB()
        ok = ctypes.windll.crypt32.CryptUnprotectData(
            ctypes.byref(_BLOB.of(blob)), None,
            None, None, None, CRYPTPROTECT_UI_FORBIDDEN, ctypes.byref(out),
        )
        if not ok:
            # 换了账户或者换了机器就会走到这儿。返回空串，界面上看起来就是「没填密钥」，
            # 比返回一串解不开的乱码去调接口要好。
            print("[dpapi] 解密失败：这份配置是在别的 Windows 账户下加密的")
            return ""
        return out.take().decode("utf-8")
    except Exception as exc:
        print(f"[dpapi] 解密失败：{exc}")
        return ""
