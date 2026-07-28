"""PyInstaller 运行时钩子：赶在 PySide6 之前把 onnxruntime 加载起来。

为什么需要它：
    onnxruntime 的原生扩展只有在 PySide6 之前加载才正常，反过来会报
    "DLL load failed while importing onnxruntime_pybind11_state"。
    而 PyInstaller 会自动生成 pyi_rth_pyside6 钩子，它在入口脚本之前就 import 了
    PySide6——所以光在 main.py 里预载是抢不过它的，必须用运行时钩子。
    自定义运行时钩子会排在自动生成的钩子前面（见 PyInstaller 的 analyze_runtime_hooks）。

只有配置里选了 RapidOCR 才真的加载，否则白白多花几百毫秒和上百 MB 内存。
"""
import json
import os
import sys


def _wanted() -> bool:
    path = os.path.join(os.environ.get("APPDATA", ""), "ScreenTranslate", "config.json")
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh).get("ocr", {}).get("engine") == "rapidocr"
    except Exception:
        return False


if _wanted():
    try:
        base = getattr(sys, "_MEIPASS", None)
        if base:
            capi = os.path.join(base, "onnxruntime", "capi")
            if os.path.isdir(capi):
                try:
                    os.add_dll_directory(capi)
                except OSError:
                    pass
        import onnxruntime  # noqa: F401
    except Exception:
        pass  # 加载不了就让 rapid_ocr 去报那个能看懂的错
