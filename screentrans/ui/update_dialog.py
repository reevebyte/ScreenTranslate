"""User-facing, browser-only GitHub Release update window."""
from __future__ import annotations

import logging
from datetime import datetime
from threading import Event, Lock, Thread

from PySide6.QtCore import QObject, Qt, QUrl, Signal
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (
    QApplication,
    QFrame,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from .. import __version__
from ..error_logging import report_exception
from ..updater import UpdateCancelled, UpdateError, UpdateInfo, check_for_update
from . import glyphs
from .icon import make_icon
from .style import BAD, OK_GREEN, TEXT, TEXT_DIM, WARN, build_qss


SHUTDOWN_WAIT_MS = 2500


def _format_size(size: int) -> str:
    value = float(size)
    units = ("B", "KB", "MB", "GB", "TB", "PB", "EB")
    for unit in units:
        if value < 1024 or unit == units[-1]:
            if unit == "B":
                return f"{size} B"
            return f"{value:.1f} {unit}"
        value /= 1024
    return f"{size} B"


def _format_published_at(value: str) -> str:
    value = value.strip()
    if not value:
        return ""
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return value
    if parsed.tzinfo is not None:
        parsed = parsed.astimezone()
        return f"{parsed:%Y-%m-%d %H:%M}（本地时间）"
    return f"{parsed:%Y-%m-%d %H:%M}"


class _UpdateThread(QObject):
    checked = Signal(object)
    failed = Signal(str)
    finished = Signal(object)

    def __init__(
        self,
        mode: str,
        *,
        url: str = "",
        repository_url: str = "",
        channel: str = "stable",
    ):
        super().__init__()
        self.mode = mode
        self.url = url
        self.repository_url = repository_url
        self.channel = channel
        self.cancel_event = Event()
        self._response_lock = Lock()
        self._response = None
        self._signal_lock = Lock()
        self._thread_lock = Lock()
        self._native_thread: Thread | None = None

    def start(self) -> None:
        with self._thread_lock:
            if self._native_thread is not None:
                raise RuntimeError("update worker has already been started")
            thread = Thread(
                target=self._run_wrapper,
                name="ScreenTranslateUpdate-check",
                daemon=True,
            )
            self._native_thread = thread
            thread.start()

    def isRunning(self) -> bool:
        with self._thread_lock:
            thread = self._native_thread
        return thread is not None and thread.is_alive()

    def wait(self, milliseconds: int) -> bool:
        with self._thread_lock:
            thread = self._native_thread
        if thread is None:
            return True
        timeout = None if milliseconds < 0 else milliseconds / 1000
        thread.join(timeout)
        return not thread.is_alive()

    def _run_wrapper(self) -> None:
        try:
            self.run()
        finally:
            self.finished.emit(self)

    def _is_cancelled(self) -> bool:
        with self._signal_lock:
            return self.cancel_event.is_set()

    def _emit_if_active(self, signal, *args: object) -> bool:
        with self._signal_lock:
            if self.cancel_event.is_set():
                return False
            signal.emit(*args)
            return True

    def _observe_response(self, response: object | None) -> None:
        with self._response_lock:
            self._response = response
            cancelled = self.cancel_event.is_set()
        if cancelled and response is not None:
            close = getattr(response, "close", None)
            if callable(close):
                close()

    def cancel(self) -> None:
        with self._signal_lock:
            self.cancel_event.set()
        with self._response_lock:
            response = self._response
        close = getattr(response, "close", None)
        if callable(close):
            try:
                close()
            except Exception:
                pass

    def run(self) -> None:
        try:
            if self.mode != "check":
                raise UpdateError("更新线程只允许检查版本")
            info = check_for_update(
                self.url,
                repository_url=self.repository_url,
                channel=self.channel,
                cancel_event=self.cancel_event,
                response_observer=self._observe_response,
            )
            self._emit_if_active(self.checked, info)
        except UpdateCancelled:
            return
        except Exception as exc:
            if self._is_cancelled():
                return
            report_exception(logging.getLogger("screentrans.errors"), "update.check", exc)
            message = str(exc) if isinstance(exc, UpdateError) else f"更新检查失败（{type(exc).__name__}）"
            self._emit_if_active(self.failed, message)
        finally:
            self._observe_response(None)


class UpdateDialog(QWidget):
    updateAvailable = Signal(object)
    releaseOpened = Signal()

    def __init__(self, cfg):
        super().__init__()
        self.cfg = cfg
        self._thread: _UpdateThread | None = None
        self._cancelled_thread: _UpdateThread | None = None
        self._available: UpdateInfo | None = None
        self._silent_check = False
        self._state = "idle"

        accent = cfg.get("appearance.accent", "#28C76F")
        repository_url = str(cfg.get("updates.repository_url", "") or "").strip()
        channel = str(cfg.get("updates.channel", "stable") or "stable").strip()
        channel_name = "稳定版" if channel == "stable" else "预览版"

        self.setObjectName("Root")
        self.setWindowTitle("ScreenTranslate · 检查更新")
        self.setMinimumWidth(560)
        self.resize(580, 260)
        self.setStyleSheet(build_qss(accent))

        layout = QVBoxLayout(self)
        layout.setContentsMargins(24, 22, 24, 20)
        layout.setSpacing(16)

        header = QHBoxLayout()
        header.setSpacing(12)
        mark = QLabel()
        mark.setPixmap(make_icon(accent, 38).pixmap(38, 38))
        mark.setFixedSize(38, 38)
        header.addWidget(mark)

        heading = QVBoxLayout()
        heading.setContentsMargins(0, 0, 0, 0)
        heading.setSpacing(2)
        title = QLabel("ScreenTranslate")
        title.setObjectName("PageTitle")
        heading.addWidget(title)
        version = QLabel(f"当前版本 {__version__}  ·  {channel_name}")
        version.setObjectName("PageSub")
        heading.addWidget(version)
        header.addLayout(heading, 1)
        layout.addLayout(header)

        self.state_card = QFrame()
        self.state_card.setObjectName("Card")
        self.state_card.setSizePolicy(
            QSizePolicy.Policy.Preferred,
            QSizePolicy.Policy.Fixed,
        )
        state_layout = QHBoxLayout(self.state_card)
        state_layout.setContentsMargins(17, 15, 17, 15)
        state_layout.setSpacing(14)

        self.status_icon = QLabel()
        self.status_icon.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.status_icon.setFixedSize(32, 32)
        state_layout.addWidget(self.status_icon, 0, Qt.AlignmentFlag.AlignTop)

        state_text = QVBoxLayout()
        state_text.setContentsMargins(0, 0, 0, 0)
        state_text.setSpacing(4)
        self.status_title = QLabel()
        self.status_title.setObjectName("UpdateStateTitle")
        state_text.addWidget(self.status_title)
        self.status = QLabel()
        self.status.setObjectName("UpdateStateText")
        self.status.setWordWrap(True)
        self.status.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        state_text.addWidget(self.status)
        state_layout.addLayout(state_text, 1)
        layout.addWidget(self.state_card)

        self.details = QFrame()
        self.details.setObjectName("Card")
        self.details.setSizePolicy(
            QSizePolicy.Policy.Preferred,
            QSizePolicy.Policy.Fixed,
        )
        detail_layout = QFormLayout(self.details)
        detail_layout.setContentsMargins(17, 14, 17, 14)
        detail_layout.setHorizontalSpacing(14)
        detail_layout.setVerticalSpacing(9)
        detail_layout.setFieldGrowthPolicy(QFormLayout.FieldGrowthPolicy.AllNonFixedFieldsGrow)

        self.published_key = QLabel("发布时间")
        self.published_value = QLabel()
        self.published_value.setTextFormat(Qt.TextFormat.PlainText)
        self.published_value.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        detail_layout.addRow(self.published_key, self.published_value)

        self.artifact_value = QLabel()
        self.artifact_value.setTextFormat(Qt.TextFormat.PlainText)
        self.artifact_value.setWordWrap(True)
        self.artifact_value.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        detail_layout.addRow("安装包", self.artifact_value)

        hash_row = QHBoxLayout()
        hash_row.setContentsMargins(0, 0, 0, 0)
        hash_row.setSpacing(7)
        self.sha256_edit = QLineEdit()
        self.sha256_edit.setReadOnly(True)
        self.sha256_edit.setPlaceholderText("SHA-256")
        hash_row.addWidget(self.sha256_edit, 1)
        self.copy_sha_btn = QPushButton("复制")
        self.copy_sha_btn.setObjectName("Ghost")
        self.copy_sha_btn.setIcon(glyphs.icon("copy", 16, "#B7BDC6"))
        self.copy_sha_btn.setToolTip("复制安装包的 SHA-256")
        self.copy_sha_btn.setMinimumWidth(64)
        self.copy_sha_btn.clicked.connect(self._copy_sha256)
        hash_row.addWidget(self.copy_sha_btn)
        detail_layout.addRow("SHA-256", hash_row)
        self.details.setVisible(False)
        layout.addWidget(self.details)
        layout.addStretch(1)

        actions = QHBoxLayout()
        actions.setSpacing(9)
        self.source = QLabel(
            f"GitHub · {repository_url.removeprefix('https://github.com/')}"
            if repository_url
            else "本地开发构建"
        )
        self.source.setObjectName("Hint")
        self.source.setToolTip(repository_url)
        self.source.setSizePolicy(
            QSizePolicy.Policy.Ignored,
            QSizePolicy.Policy.Preferred,
        )
        self.source.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        actions.addWidget(self.source, 1)
        self.check_btn = QPushButton("检查更新")
        self.check_btn.setObjectName("Primary")
        self.check_btn.setIcon(glyphs.icon("retry", 16, "#0E1013"))
        self.check_btn.setFixedWidth(112)
        self.check_btn.setFixedHeight(36)
        self.check_btn.clicked.connect(self.check)
        actions.addWidget(self.check_btn)
        self.release_btn = QPushButton("打开发布页")
        self.release_btn.setVisible(False)
        self.release_btn.setIcon(glyphs.icon("globe", 16, TEXT))
        self.release_btn.setFixedSize(128, 36)
        self.release_btn.clicked.connect(self.open_release)
        actions.addWidget(self.release_btn)
        layout.addLayout(actions)

        if repository_url:
            self._set_state(
                "idle",
                "准备检查更新",
                f"将从 GitHub Release 获取{channel_name}发布信息。",
            )
        else:
            self._set_state(
                "unconfigured",
                "此构建未配置更新",
                "请使用 GitHub Release 中的正式安装版或便携版。",
            )

    def _set_state(self, state: str, title: str, message: str) -> None:
        visuals = {
            "idle": ("info", TEXT_DIM),
            "checking": ("retry", self.cfg.get("appearance.accent", "#28C76F")),
            "current": ("check", OK_GREEN),
            "available": ("info", self.cfg.get("appearance.accent", "#28C76F")),
            "error": ("info", BAD),
            "unconfigured": ("info", WARN),
        }
        glyph, color = visuals.get(state, visuals["idle"])
        self._state = state
        self.status_icon.setPixmap(glyphs.pixmap(glyph, 26, color))
        self.status_title.setText(title)
        self.status_title.setStyleSheet(f"color: {color};")
        self.status.setText(message)

    def _idle_button_text(self) -> str:
        if self._state == "error":
            return "重试"
        if self._state in ("current", "available"):
            return "重新检查"
        return "检查更新"

    def _source(self) -> tuple[str, str, str]:
        return (
            str(self.cfg.get("updates.manifest_url", "") or "").strip(),
            str(self.cfg.get("updates.repository_url", "") or "").strip(),
            str(self.cfg.get("updates.channel", "stable") or "stable").strip(),
        )

    def _set_busy(self, busy: bool) -> None:
        self.check_btn.setEnabled(not busy)
        self.release_btn.setEnabled(not busy)
        self.check_btn.setText("检查中…" if busy else self._idle_button_text())
        icon_color = TEXT_DIM if busy else "#0E1013"
        self.check_btn.setIcon(glyphs.icon("retry", 16, icon_color))

    def _start(self, worker: _UpdateThread) -> None:
        if self._thread is not None and self._thread.isRunning():
            return
        self._thread = worker
        self._cancelled_thread = None
        worker.failed.connect(self._failed)
        worker.finished.connect(self._finished)
        self._set_busy(True)
        worker.start()

    def _begin_check(self, *, silent: bool) -> None:
        manifest_url, repository_url, channel = self._source()
        self._silent_check = silent
        self._available = None
        self.details.setVisible(False)
        self.published_value.clear()
        self.artifact_value.clear()
        self.sha256_edit.clear()
        self.copy_sha_btn.setText("复制")
        self.release_btn.setVisible(False)
        self.release_btn.setText("打开发布页")
        if not manifest_url or not repository_url:
            if not silent:
                self._set_state(
                    "unconfigured",
                    "此构建未配置更新",
                    "请使用 GitHub Release 中的正式安装版或便携版。",
                )
                self._set_busy(False)
            return
        if not silent:
            channel_name = "稳定版" if channel == "stable" else "预览版"
            self._set_state(
                "checking",
                "正在检查更新",
                f"正在读取 GitHub 上的{channel_name}发布信息，请稍候。",
            )
        worker = _UpdateThread(
            "check",
            url=manifest_url,
            repository_url=repository_url,
            channel=channel,
        )
        worker.checked.connect(self._checked)
        self._start(worker)

    def check(self, _checked: bool = False) -> None:
        self._begin_check(silent=False)

    def check_silently(self) -> None:
        if not (self._thread is not None and self._thread.isRunning()):
            self._begin_check(silent=True)

    def _checked(self, info: object) -> None:
        if info is None:
            if not self._silent_check:
                channel = str(self.cfg.get("updates.channel", "stable") or "stable").strip()
                channel_name = "稳定版" if channel == "stable" else "预览版"
                self._set_state(
                    "current",
                    "已经是最新版",
                    f"当前安装的 {__version__} 已是{channel_name}的最新版本。",
                )
            return
        if not isinstance(info, UpdateInfo):
            self._set_state(
                "error",
                "检查失败",
                "更新服务器返回了无法识别的数据。",
            )
            return
        self._available = info
        self.updateAvailable.emit(info)
        channel_name = "稳定版" if info.channel == "stable" else "预览版"
        self._set_state(
            "available",
            f"发现新版本 {info.version}",
            f"这是{channel_name}。请前往 GitHub 发布页面查看并手动下载安装。",
        )
        published_at = _format_published_at(info.published_at)
        self.published_key.setVisible(bool(published_at))
        self.published_value.setVisible(bool(published_at))
        self.published_value.setText(published_at)
        self.artifact_value.setText(
            f"{info.artifact.name}（{_format_size(info.artifact.size)}）"
        )
        self.sha256_edit.setText(info.artifact.sha256)
        self.copy_sha_btn.setText("复制")
        self.details.setVisible(True)
        self.release_btn.setText("打开发布页")
        self.release_btn.setVisible(True)

    def _copy_sha256(self, _checked: bool = False) -> None:
        if self._available is None:
            return
        QApplication.clipboard().setText(self._available.artifact.sha256)
        self.copy_sha_btn.setText("已复制")

    def open_release(self) -> None:
        if self._available is None:
            return
        try:
            if not QDesktopServices.openUrl(QUrl(self._available.release_url)):
                raise UpdateError("系统浏览器未接受发布页面地址")
            self.status.setText("已交给系统浏览器打开 GitHub 发布页面。")
            self.releaseOpened.emit()
        except Exception as exc:
            report_exception(logging.getLogger("screentrans.errors"), "update.open_release", exc)
            message = str(exc) if isinstance(exc, UpdateError) else f"无法打开发布页面（{type(exc).__name__}）"
            self.status.setText(message)

    def _failed(self, message: str) -> None:
        if not self._silent_check:
            self._set_state("error", "检查失败", message)

    def _finished(self, worker: _UpdateThread) -> None:
        if self._thread is not worker:
            return
        self._set_busy(False)
        self._thread = None
        self._cancelled_thread = None
        self._silent_check = False
        self.check_btn.setText(self._idle_button_text())
        self.check_btn.setIcon(glyphs.icon("retry", 16, "#0E1013"))

    def shutdown(self) -> bool:
        worker = self._thread
        if worker is None:
            return True
        if self._cancelled_thread is worker:
            return not worker.isRunning()
        self._cancelled_thread = worker
        worker.cancel()
        if not worker.isRunning():
            return True
        return worker.wait(SHUTDOWN_WAIT_MS)

    def closeEvent(self, event) -> None:
        self.shutdown()
        super().closeEvent(event)

    def show_front(self) -> None:
        self.show()
        self.setWindowState(
            (self.windowState() & ~Qt.WindowState.WindowMinimized) | Qt.WindowState.WindowActive
        )
        self.raise_()
        self.activateWindow()
