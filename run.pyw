"""无控制台窗口启动（双击此文件，或用 pythonw run.pyw）。"""
import os
import sys

# NumPy is optional (RapidOCR only). Its bundled OpenBLAS otherwise creates one
# worker per logical CPU at import time, reserving hundreds of MiB while idle.
os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

try:
    from screentrans.error_logging import install_exception_hooks

    _error_hooks = install_exception_hooks()
except Exception:
    # 日志目录不可写不能反过来阻止程序启动。
    _error_hooks = None

from screentrans.main import main

if __name__ == "__main__":
    sys.exit(main())
