"""Privacy-conscious rotating error logs.

The bootstrap installs the process and thread hooks. Callers should log only operation
names and exceptions, never screenshots, OCR text, translations, or request/response
bodies.
"""
from __future__ import annotations

import logging
import os
import re
import sys
import threading
from dataclasses import dataclass
from logging.handlers import RotatingFileHandler
from pathlib import Path
from types import TracebackType
from typing import Callable


DEFAULT_MAX_BYTES = 512 * 1024
DEFAULT_BACKUP_COUNT = 3

_REDACTIONS = (
    (re.compile(r"(?i)(authorization\s*[:=]\s*)(?:bearer|basic)\s+[^\s,;]+"), r"\1<redacted>"),
    (
        re.compile(
            r"(?i)((?<![A-Za-z0-9_])(?:['\"])?(?:api[_-]?key|key|[A-Za-z0-9_-]*[_-]key|"
            r"token|[A-Za-z0-9_-]*[_-]token|secret|[A-Za-z0-9_-]*[_-]secret|password)"
            r"(?:['\"])?\s*[:=]\s*)"
            r"(?:['\"])?[^\s,'\";&]+"
        ),
        r"\1<redacted>",
    ),
    (re.compile(r"(?i)\b(?:sk|nvapi)-[A-Za-z0-9_-]{8,}\b"), "<redacted-key>"),
    (re.compile(r"\bAIza[0-9A-Za-z_-]{20,}\b"), "<redacted-key>"),
    (re.compile(r"(?i)dpapi:v1:[A-Za-z0-9+/=_-]+"), "<redacted-dpapi>"),
    (re.compile(r"(?i)data:image/[^;,\s]+;base64,[A-Za-z0-9+/=\r\n]+"), "<redacted-image>"),
    (re.compile(r"(?i)(https://)[^/@\s:]+:[^/@\s]+@"), r"\1<redacted>@"),
    (
        re.compile(r"(?i)([?&](?:key|api_key|token|access_token)=)[^&#\s]+"),
        r"\1<redacted>",
    ),
)


def default_log_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA") or os.environ.get("APPDATA")
    if base:
        return Path(base) / "ScreenTranslate" / "logs"
    return Path.cwd() / "logs"


def redact(text: object) -> str:
    sanitized = str(text)
    for pattern, replacement in _REDACTIONS:
        sanitized = pattern.sub(replacement, sanitized)
    return sanitized


class RedactingFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        return redact(super().format(record))


def configure_error_logger(
    log_dir: Path | None = None,
    *,
    max_bytes: int = DEFAULT_MAX_BYTES,
    backup_count: int = DEFAULT_BACKUP_COUNT,
    logger_name: str = "screentrans.errors",
) -> logging.Logger:
    if max_bytes < 1:
        raise ValueError("max_bytes must be positive")
    if backup_count < 0:
        raise ValueError("backup_count cannot be negative")

    destination = Path(log_dir) if log_dir is not None else default_log_dir()
    destination.mkdir(parents=True, exist_ok=True)

    logger = logging.getLogger(logger_name)
    logger.setLevel(logging.ERROR)
    logger.propagate = False
    for handler in list(logger.handlers):
        handler.close()
        logger.removeHandler(handler)

    handler = RotatingFileHandler(
        destination / "errors.log",
        maxBytes=max_bytes,
        backupCount=backup_count,
        encoding="utf-8",
        delay=True,
    )
    handler.setFormatter(
        RedactingFormatter(
            "%(asctime)s %(levelname)s %(name)s %(message)s",
            datefmt="%Y-%m-%dT%H:%M:%S",
        )
    )
    logger.addHandler(handler)
    return logger


def report_exception(logger: logging.Logger, operation: str, exc: BaseException) -> None:
    """Record stack locations without exception values or user/API payloads."""
    safe_operation = re.sub(r"[^A-Za-z0-9_.:/ -]", "?", operation)[:120]
    locations: list[str] = []
    traceback = exc.__traceback__
    while traceback is not None:
        code = traceback.tb_frame.f_code
        locations.append(f"{Path(code.co_filename).name}:{traceback.tb_lineno}:{code.co_name}")
        traceback = traceback.tb_next
    location_text = " <- ".join(locations[-12:]) or "no-traceback"
    logger.error("%s failed: %s at %s", safe_operation, type(exc).__name__, location_text)


@dataclass
class InstalledExceptionHooks:
    previous_sys_hook: Callable[[type[BaseException], BaseException, TracebackType | None], None]
    previous_thread_hook: Callable[[object], None] | None
    sys_hook: Callable[[type[BaseException], BaseException, TracebackType | None], None]
    thread_hook: Callable[[object], None] | None

    def restore(self) -> None:
        if sys.excepthook is self.sys_hook:
            sys.excepthook = self.previous_sys_hook
        if (
            self.thread_hook is not None
            and hasattr(threading, "excepthook")
            and threading.excepthook is self.thread_hook
            and self.previous_thread_hook is not None
        ):
            threading.excepthook = self.previous_thread_hook


def install_exception_hooks(
    logger: logging.Logger | None = None,
    *,
    log_dir: Path | None = None,
) -> InstalledExceptionHooks:
    """Install process/thread hooks while preserving Python's existing behavior."""
    error_logger = logger or configure_error_logger(log_dir)
    previous_sys_hook = sys.excepthook
    previous_thread_hook = getattr(threading, "excepthook", None)

    def sys_hook(
        exc_type: type[BaseException],
        exc: BaseException,
        traceback: TracebackType | None,
    ) -> None:
        if not issubclass(exc_type, (KeyboardInterrupt, SystemExit)):
            try:
                if exc.__traceback__ is None and traceback is not None:
                    exc = exc.with_traceback(traceback)
                report_exception(error_logger, "process.unhandled", exc)
            except Exception:
                pass
        previous_sys_hook(exc_type, exc, traceback)

    def thread_hook(args: object) -> None:
        exc_type = getattr(args, "exc_type", None)
        exc = getattr(args, "exc_value", None)
        if (
            isinstance(exc_type, type)
            and isinstance(exc, BaseException)
            and not issubclass(exc_type, (KeyboardInterrupt, SystemExit))
        ):
            try:
                report_exception(error_logger, "thread.unhandled", exc)
            except Exception:
                pass
        if previous_thread_hook is not None:
            previous_thread_hook(args)

    sys.excepthook = sys_hook
    installed_thread_hook = None
    if previous_thread_hook is not None:
        installed_thread_hook = thread_hook
        threading.excepthook = thread_hook
    return InstalledExceptionHooks(
        previous_sys_hook,
        previous_thread_hook,
        sys_hook,
        installed_thread_hook,
    )
