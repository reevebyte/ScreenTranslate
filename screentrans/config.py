"""配置读写。配置文件位于 %APPDATA%\\ScreenTranslate\\config.json"""
from __future__ import annotations

import copy
import json
import os
import sys
from pathlib import Path

from . import dpapi

APP_NAME = "ScreenTranslate"
APP_TITLE = "划词截屏翻译"

CONFIG_DIR = Path(os.environ.get("APPDATA") or Path.home()) / APP_NAME
CONFIG_PATH = CONFIG_DIR / "config.json"
_BUNDLED_UPDATE_SOURCE_NAME = "screentrans-update-source.json"


def _load_bundled_update_source() -> dict[str, str]:
    """Load the immutable update source embedded by release builds.

    Source checkouts and ordinary local builds intentionally have no default
    update source.  A frozen release may only override the three non-secret
    update fields below.
    """
    if not getattr(sys, "frozen", False):
        return {}
    bundle_root = Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))
    try:
        raw = json.loads(
            (bundle_root / _BUNDLED_UPDATE_SOURCE_NAME).read_text(encoding="utf-8")
        )
    except (FileNotFoundError, OSError, UnicodeError, json.JSONDecodeError):
        return {}
    if not isinstance(raw, dict):
        return {}
    source = {
        "manifest_url": raw.get("manifest_url"),
        "repository_url": raw.get("repository_url"),
        "channel": raw.get("channel"),
    }
    if not all(isinstance(value, str) for value in source.values()):
        return {}
    return source


_BUNDLED_UPDATE_SOURCE = _load_bundled_update_source()

# 全局快捷键清单：(内部名, 配置项, 默认值, 界面上的叫法, 说明)
# 放在 config 里而不是 main 里，是为了让设置界面也能直接用——
# 设置界面属于 ui 包，main 又要 import ui，写在 main 里会绕成循环导入。
HOTKEYS = [
    ("capture", "hotkey", "Ctrl+Alt+Q", "框选翻译", "按下后拉框，松手立刻翻译"),
    ("toggle", "hotkey_toggle", "Ctrl+Alt+W", "收起 / 显示", "同一个键来回切：译文开着就收进托盘，收着就叫回来"),
]

DEFAULTS: dict = {
    "hotkey": "Ctrl+Alt+Q",           # 框选翻译
    "hotkey_toggle": "Ctrl+Alt+W",    # 把译文缩到托盘 / 再叫回来（同一个键来回切）
    "ocr": {
        # windows = 系统离线 OCR；azure/youdao = 上传截图；rapidocr = 可选离线模型
        "engine": "windows",
        # 按顺序尝试，取识别字符最多的结果。只留一个可以更快。
        "languages": ["zh-Hans-CN", "en-US"],
        "upscale": True,          # 小图先放大再识别，显著提高小字准确率
        "azure_vision": {
            "endpoint": "",
            "key": "",
        },
    },
    "translator": {
        "provider": "microsoft",
        "microsoft": {
            "key": "",
            "region": "eastasia",
            "endpoint": "https://api.cognitive.microsofttranslator.com",
        },
        "google": {"key": ""},
        "google_free": {},
        "bing_free": {},
        "microsoft_free": {},
        "tencent_free": {},
        "yandex_free": {},
        "iciba_free": {},
        "deepl": {"key": "", "free_plan": True},
        "openai": {
            "base_url": "https://api.deepseek.com/v1",
            "key": "",
            "model": "deepseek-chat",
        },
        "nvidia": {
            "base_url": "https://integrate.api.nvidia.com/v1",
            "key": "",
            "model": "",
        },
        "anthropic": {
            "base_url": "https://api.anthropic.com",
            "key": "",
            "model": "claude-sonnet-5",
        },
    },
    "lang": {
        # 识别为中文时翻译成什么；其余语言一律翻译成中文
        "zh_target": "en",
    },
    "appearance": {
        "font_family": "Microsoft YaHei UI",
        "min_font_px": 9,
        "auto_copy": True,        # 译文自动进剪贴板
        "close_mode": "click",    # click | timeout | leave
        "timeout_ms": 5000,       # close_mode = timeout 时生效
        "accent": "#28C76F",
    },
    "updates": {
        # 正式包由 CI 嵌入并固定到同一 GitHub 仓库；源码运行默认不联网检查。
        "manifest_url": _BUNDLED_UPDATE_SOURCE.get("manifest_url", ""),
        "repository_url": _BUNDLED_UPDATE_SOURCE.get("repository_url", ""),
        "channel": _BUNDLED_UPDATE_SOURCE.get("channel", "stable"),
    },
    "autostart": False,
}

# 换过的旧默认值。命中就跟着换成新默认——这些值当初没有任何界面能改，
# 盘上留着的必然是「从来没动过」，不是用户挑的，没必要替他一直守着。
_STALE_DEFAULTS = {"appearance.accent": {"#4C8DFF"}}


def _secret_options(data: dict):
    yield from (data.get("translator") or {}).values()
    azure = (data.get("ocr") or {}).get("azure_vision")
    if isinstance(azure, dict):
        yield azure


def _map_secrets(data: dict, fn) -> None:
    """就地把翻译接口和云端 OCR 的 key 字段过一遍 fn（加密或解密）。

    所有密钥字段都叫 key；这里不处理 Endpoint 等非秘密配置。
    这里不去 import translators 拿 FIELDS，是为了不让 config 反向依赖上层模块。
    """
    for opts in _secret_options(data):
        if isinstance(opts, dict) and isinstance(opts.get("key"), str):
            opts["key"] = fn(opts["key"])


def _encrypt_secret(text: str) -> str:
    """防御性确认加密器没有把非空密钥原样返回。"""
    encrypted = dpapi.encrypt(text)
    if text and not text.startswith(dpapi.PREFIX) and not encrypted.startswith(dpapi.PREFIX):
        raise dpapi.EncryptionError("DPAPI 未返回密文，密钥未保存")
    return encrypted


def _deep_merge(base: dict, override: dict) -> dict:
    out = copy.deepcopy(base)
    for k, v in (override or {}).items():
        if isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = _deep_merge(out[k], v)
        else:
            out[k] = v
    return out


class Config:
    def __init__(self) -> None:
        self.data: dict = copy.deepcopy(DEFAULTS)
        self.load()

    def load(self) -> None:
        try:
            raw = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
            self.data = _deep_merge(DEFAULTS, raw)
            # 更新仓库是构建时的信任边界，不属于用户配置。正式包固定到嵌入值；
            # 源码和普通本地构建固定为空，旧配置也不能让它们自行联网。
            self.data["updates"] = copy.deepcopy(DEFAULTS["updates"])
            # 盘上还有明文密钥就地升级一次。不主动升的话，
            # 老用户除非碰巧去改一次设置，否则密钥永远是明文躺在那儿。
            stale = any(
                isinstance(o, dict) and isinstance(o.get("key"), str)
                and o["key"] and not o["key"].startswith(dpapi.PREFIX)
                for o in _secret_options(self.data)
            )
            _map_secrets(self.data, dpapi.decrypt)
            moved = self._retire_stale_defaults()
            if moved or (stale and dpapi.available()):
                try:
                    self.save()
                except Exception as exc:
                    print(f"[config] 配置升级没存下来（不影响使用）：{exc}")
        except FileNotFoundError:
            pass
        except Exception as exc:  # 配置损坏时不要拖垮启动
            print(f"[config] 读取失败，使用默认值: {exc}")
            self.data = copy.deepcopy(DEFAULTS)

    def _retire_stale_defaults(self) -> bool:
        """盘上还留着**旧版默认值**的项，跟着换成新默认值。返回有没有换过。

        只对「当初根本没有界面能改的项」这么做。那种值躺在配置里必然是从来没动过，
        不是用户挑的——为它一直守着，等于新默认值永远轮不到老用户。
        真动过手的（值既不等于旧默认也不等于新默认）一律不碰。
        """
        moved = False
        for path, olds in _STALE_DEFAULTS.items():
            if self.get(path) in olds:
                fresh = DEFAULTS
                for part in path.split("."):
                    fresh = fresh[part]
                self.set(path, fresh)
                moved = True
        return moved

    def save(self) -> None:
        """写盘。先写临时文件再原子替换，写一半断电也不会留下半个损坏的配置。

        替换失败时退回直接覆盖：杀毒软件锁文件、同步盘（OneDrive）占用、
        某些沙箱的文件重定向层，都可能让 os.replace 失败。
        那种情况下牺牲原子性也要把配置存下来，总比存不下强。
        """
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        # 内存里一直是明文（各处直接 get 就能用），只在写盘这一刻加密
        on_disk = copy.deepcopy(self.data)
        _map_secrets(on_disk, _encrypt_secret)
        payload = json.dumps(on_disk, ensure_ascii=False, indent=2)
        tmp = CONFIG_PATH.with_suffix(".json.tmp")
        try:
            tmp.write_text(payload, encoding="utf-8")
            tmp.replace(CONFIG_PATH)
            return
        except OSError:
            pass
        try:
            tmp.unlink()
        except OSError:
            pass
        CONFIG_PATH.write_text(payload, encoding="utf-8")

    def get(self, path: str, default=None):
        """config.get('translator.microsoft.key')"""
        node = self.data
        for part in path.split("."):
            if not isinstance(node, dict) or part not in node:
                return default
            node = node[part]
        return node

    def set(self, path: str, value) -> None:
        parts = path.split(".")
        node = self.data
        for part in parts[:-1]:
            node = node.setdefault(part, {})
        node[parts[-1]] = value
