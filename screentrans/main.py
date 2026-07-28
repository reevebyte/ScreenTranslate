"""程序入口：托盘常驻，按快捷键框选，松手即翻译。"""
from __future__ import annotations

import ctypes
import copy
import logging
import os
import sys
import time
import weakref

from . import winsys

# DPI 感知必须赶在 QApplication 之前设置，否则多屏 / 缩放不是 100% 时会截错位置
winsys.enable_dpi_awareness()


def _preload_onnxruntime() -> None:
    """RapidOCR 依赖的 onnxruntime 必须在 PySide6 之前导入。

    PySide6 会往 DLL 搜索路径里插自己的目录，之后再导入 onnxruntime 就会
    在加载原生扩展时失败（报一个看不懂的 ImportError）。反过来先导入就没事。
    只在配置选了 rapidocr 时才预载，免得没用到也白白多花几百毫秒和上百 MB 内存。
    """
    import importlib
    import importlib.util

    from .config import Config

    try:
        if Config().get("ocr.engine") != "rapidocr":
            return
        if importlib.util.find_spec("onnxruntime") is None:
            return
        importlib.import_module("onnxruntime")
    except Exception as exc:
        print(f"[main] 预载 onnxruntime 失败：{exc}")


_preload_onnxruntime()

from PySide6.QtCore import QRect, Qt, QTimer  # noqa: E402
from PySide6.QtGui import QAction  # noqa: E402
from PySide6.QtWidgets import QApplication, QDialog, QMenu, QSystemTrayIcon  # noqa: E402

from .config import APP_TITLE, HOTKEYS, Config  # noqa: E402
from .hotkey import HotkeyHost  # noqa: E402
from .error_logging import report_exception  # noqa: E402
from .overlay import OverlayManager  # noqa: E402
from .result import ResultWindow  # noqa: E402
from .ui.icon import make_icon  # noqa: E402
from .ui import glyphs  # noqa: E402
from .ui.style import build_qss  # noqa: E402

MUTEX_NAME = "Global\\ScreenTranslate.SingleInstance.v1"
MAX_CONCURRENT_TRANSLATIONS = 2
WORKER_SHUTDOWN_WAIT_MS = 2500


def _already_running(wait_seconds: float = 0.0) -> bool:
    """单实例检查。

    重启时新进程可能比旧进程退得还快，互斥体还没释放，
    新的自己就会被当成「重复启动」挡在门外——所以重启场景要给一点时间等它让位。
    """
    kernel32 = ctypes.windll.kernel32
    deadline = time.monotonic() + wait_seconds
    while True:
        handle = kernel32.CreateMutexW(None, False, MUTEX_NAME)
        if not (handle and ctypes.GetLastError() == 183):  # ERROR_ALREADY_EXISTS
            return False                                   # 句柄要一直留着，别关
        kernel32.CloseHandle(handle)
        if time.monotonic() >= deadline:
            return True
        time.sleep(0.2)


class App:
    def __init__(self, qapp: QApplication):
        self.qapp = qapp
        self._shutting_down = False
        self.cfg = Config()
        try:
            winsys.ensure_autostart(bool(self.cfg.get("autostart", False)))
        except Exception as exc:
            # 自启项损坏不能阻止程序本身启动；设置页仍会把它显示为未启用。
            print(f"[main] 修复开机自启路径失败：{exc}")
        self.accent = self.cfg.get("appearance.accent", "#28C76F")

        self.overlays = OverlayManager(self.accent)
        self.result: ResultWindow | None = None
        self.minimized_result: ResultWindow | None = None
        self.worker: TranslateWorker | None = None
        # 最多允许一个正在取消的旧请求和一个当前请求；更密集的操作只保留最后一次。
        # 请求序号负责挡住旧任务迟到的 done/failed 信号。
        self._workers: list[TranslateWorker] = []
        self._queued_worker: dict | None = None
        self._request_serial = 0
        # 边框松手后要等合成器把透明帧画上去再截屏；连续拖动时只认最后一次。
        self._capture_serial = 0
        self.settings: SettingsWindow | None = None
        self.updates: UpdateDialog | None = None
        self._editor = None
        self._pending: dict | None = None
        # 上一次的识别结果，「重新翻译」直接拿它复用，不用再 OCR 一遍
        self._last: dict = {}
        self._hotkey_state: dict[str, tuple[bool, str]] = {}

        self.hotkey_host = HotkeyHost()
        self.hotkey_host.triggered.connect(self._on_hotkey)

        self._build_tray()
        self.qapp.aboutToQuit.connect(self._shutdown)
        for name, key, default, _label, _desc in HOTKEYS:
            self._apply_hotkey(name, self.cfg.get(key, default), announce=False)
        QTimer.singleShot(1500, self._check_updates_silently)

    # ---------------------------------------------------------------- 托盘
    def _build_tray(self):
        self.icon = make_icon(self.accent)
        self.tray = QSystemTrayIcon(self.icon)   # 提示文字交给 _refresh_menu_labels

        menu = QMenu()
        menu.setStyleSheet(build_qss(self.accent))

        def item(glyph: str, text: str, slot):
            act = QAction(glyphs.icon(glyph, 16, "#B7BDC6"), text, menu)
            act.triggered.connect(slot)
            menu.addAction(act)
            return act

        self.act_capture = item("scan", "开始框选翻译", self.start_capture)
        self.act_restore = item("eye", "显示上次译文", self.restore_result)
        self.act_restore.setEnabled(False)
        menu.addSeparator()
        item("sliders", "设置…", self.open_settings)
        self.act_update = item("info", "检查更新…", self.open_updates)
        item("retry", "重启", self.restart)
        item("close", "退出", self.quit)

        self.tray.setContextMenu(menu)
        self.tray.activated.connect(self._on_tray_activated)
        self.tray.show()
        self._refresh_menu_labels()

    def _refresh_menu_labels(self):
        """把当前快捷键写进菜单项里——省得为了看一眼按键还要开设置。"""
        specs = {n: self.cfg.get(k, d) for n, k, d, _l, _s in HOTKEYS}
        self.act_capture.setText(f"开始框选翻译\t{specs['capture']}")
        self.act_restore.setText(f"显示上次译文\t{specs['toggle']}")
        self.tray.setToolTip(
            "\n".join([APP_TITLE] + [f"{label}：{specs[n]}" for n, _k, _d, label, _s in HOTKEYS])
        )

    def _on_tray_activated(self, reason):
        if reason == QSystemTrayIcon.ActivationReason.Trigger:
            self.start_capture()
        elif reason == QSystemTrayIcon.ActivationReason.DoubleClick:
            self.open_settings()

    def notify(self, title: str, body: str):
        self.tray.showMessage(title, body, self.icon, 3000)

    # ------------------------------------------------------------ 快捷键
    def _apply_hotkey(self, name: str, spec: str, announce: bool = True) -> None:
        label = next((l for n, _k, _d, l, _s in HOTKEYS if n == name), name)
        ok, message = self.hotkey_host.register(name, spec)
        self._hotkey_state[name] = (ok, message)
        self._refresh_menu_labels()
        if self.settings is not None:
            self.settings.set_hotkey_status(name, ok, message)
        if not ok:
            self.notify(f"{APP_TITLE} · {label}快捷键未生效", message)
        elif announce:
            self.notify(APP_TITLE, f"{label}快捷键已改为 {spec}")

    def _on_hotkey(self, name: str) -> None:
        if name == "capture":
            self.start_capture()
        elif name == "toggle":
            self.toggle_result()

    def toggle_result(self):
        """同一个键来回切：开着就缩到托盘，缩着就叫回来。"""
        if self.result is not None and self.result.isVisible():
            self.result.minimize()
        elif self.minimized_result is not None:
            self.restore_result()
        else:
            self.notify(APP_TITLE, "现在没有译文可以显示")

    # -------------------------------------------------------------- 主流程
    @staticmethod
    def _weak_sender_slot(sender, callback, *prefix):
        """Build a Qt slot without keeping its one-shot sender alive forever."""
        sender_ref = weakref.ref(sender)

        def invoke(*args):
            current = sender_ref()
            if current is not None:
                callback(current, *prefix, *args)

        return invoke

    def start_capture(self):
        if self.overlays.active:
            return
        self._dismiss_result()
        # 让上一个窗口先真正消失，再冻结屏幕，否则会把它自己截进去
        QTimer.singleShot(30, lambda: self.overlays.start(self._on_selected))

    def _dismiss_result(self):
        self._capture_serial += 1
        self._close_editor()
        self._cancel_active_worker()
        self._pending = None
        self._last = {}
        if self.result is not None:
            self.result.close()
            self.result = None
        # 开始新一次框选时，上一次缩到托盘的结果也一并丢掉，只保留一个结果窗口
        if self.minimized_result is not None:
            self.minimized_result.close()
            self._set_minimized(None)

    def _set_minimized(self, window) -> None:
        self.minimized_result = window
        self.act_restore.setEnabled(window is not None)

    def _on_minimized(self, window):
        self._close_editor(window)
        if self.result is window:
            self.result = None
        self._set_minimized(window)
        key = self.cfg.get("hotkey_toggle", "Ctrl+Alt+W")
        self.notify(APP_TITLE, f"译文已收起，按 {key} 或从托盘菜单「显示上次译文」叫回来")

    def restore_result(self):
        window = self.minimized_result
        if window is None:
            return
        self._set_minimized(None)
        self.result = window
        window.restore_from_tray()

    def _on_selected(self, screen, phys_rect, crop):
        from . import render

        logical = self._logical_rect(screen, phys_rect)
        window = ResultWindow(screen, logical, render.pil_to_qimage(crop), self.cfg)
        window.closed.connect(self._weak_sender_slot(window, self._on_result_closed))
        window.minimized.connect(self._weak_sender_slot(window, self._on_minimized))
        window.retry.connect(self._weak_sender_slot(window, self._retranslate))
        window.editRequested.connect(
            self._weak_sender_slot(window, self._open_block_editor)
        )
        window.regionChanged.connect(
            self._weak_sender_slot(window, self._on_region_changed)
        )
        window.show_at()
        self.result = window
        self._start_worker(crop, window)

    def _start_worker(self, crop, window, blocks=None, target_overrides=None):
        from .worker import TranslateWorker

        self._request_serial += 1
        request_id = self._request_serial
        # 请求即使在 OCR 或翻译阶段失败，也要留得住这张当前截图；结果窗口上的
        # 「重试」才能重新跑完整流程，而不是因为 _last 还没等到成功结果就卡住。
        previous = self._last if self._last.get("window") is window else {}
        self._last = {"image": crop, "window": window, "blocks": blocks}
        for key in ("translations", "targets", "target_overrides"):
            if key in previous:
                self._last[key] = previous[key]
        request = {
            "mode": "full",
            "id": request_id,
            "worker": None,
            "factory": TranslateWorker,
            "image": crop,
            "window": window,
            "blocks": blocks,
            "target_overrides": target_overrides,
        }
        return self._schedule_worker_request(request)

    @staticmethod
    def _cancel_worker(worker) -> None:
        cancel = getattr(worker, "cancel", None)
        if callable(cancel):
            cancel()
        else:
            worker.cancelled = True

    def _running_worker_count(self) -> int:
        count = 0
        for worker in self._workers:
            is_running = getattr(worker, "isRunning", None)
            if callable(is_running) and is_running():
                count += 1
        return count

    def _schedule_worker_request(self, request: dict) -> bool:
        """Cancel obsolete work, cap concurrency, and retain only the latest queued request."""
        for running in self._workers:
            is_running = getattr(running, "isRunning", None)
            if callable(is_running) and is_running():
                self._cancel_worker(running)
        self.worker = None
        self._pending = request
        self._queued_worker = None
        if self._running_worker_count() >= MAX_CONCURRENT_TRANSLATIONS:
            self._queued_worker = request
            return True
        return self._launch_worker_request(request)

    def _launch_worker_request(self, request: dict) -> bool:
        if getattr(self, "_shutting_down", False) or self._pending is not request:
            return False
        factory = request["factory"]
        if request["mode"] == "subset":
            worker = factory(
                request["image"],
                self.cfg,
                request["worker_blocks"],
                request["worker_targets"],
            )
        else:
            worker = factory(
                request["image"],
                self.cfg,
                request.get("blocks"),
                request.get("target_overrides"),
            )
        request["worker"] = worker
        if self._queued_worker is request:
            self._queued_worker = None
        self._workers.append(worker)
        request_id = request["id"]
        if request["mode"] == "subset":
            worker.done.connect(
                self._weak_sender_slot(
                    worker, self._on_subset_translated, request_id
                )
            )
            worker.failed.connect(
                self._weak_sender_slot(worker, self._on_subset_failed, request_id)
            )
        else:
            worker.done.connect(
                self._weak_sender_slot(worker, self._on_translated, request_id)
            )
            worker.failed.connect(
                self._weak_sender_slot(worker, self._on_failed, request_id)
            )
        worker.finished.connect(self._weak_sender_slot(worker, self._release_worker))
        self.worker = worker
        worker.start()
        return True

    def _cancel_active_worker(self) -> None:
        worker = self.worker
        if worker is not None:
            self._cancel_worker(worker)
        self.worker = None
        self._pending = None
        self._queued_worker = None

    def _release_worker(self, worker: TranslateWorker) -> None:
        try:
            self._workers.remove(worker)
        except ValueError:
            pass
        if self.worker is worker:
            self.worker = None
        delete_later = getattr(worker, "deleteLater", None)
        if callable(delete_later):
            delete_later()
        queued = self._queued_worker
        if queued is not None and self._running_worker_count() < MAX_CONCURRENT_TRANSLATIONS:
            self._launch_worker_request(queued)

    def _retranslate(self, window):
        """用当前设置重新翻一遍。文字已经认过了，直接复用，别再 OCR 一次。"""
        self._close_editor(window)
        image = self._last.get("image")
        blocks = self._last.get("blocks")
        if self._last.get("window") is not window or (image is None and not blocks):
            window.set_error("没有可重试的截图，请重新框选")
            return
        self._start_worker(
            image,
            window,
            blocks,
            self._last.get("target_overrides"),
        )

    # ---------------------------------------------------------- OCR 校对 / 单块重译
    def _open_block_editor(self, window) -> None:
        from .ui.block_editor import BlockEditorDialog

        if self._editor is not None:
            self._editor.show()
            self._editor.raise_()
            self._editor.activateWindow()
            return

        pending = self._pending
        if (
            pending
            and pending.get("mode") == "subset"
            and pending.get("window") is window
        ):
            window.editing_finished()
            self.notify(APP_TITLE, "文本块仍在重译，请稍后再打开校对窗口")
            return

        cache = self._last
        blocks = cache.get("blocks") or []
        translations = cache.get("translations") or []
        targets = cache.get("targets") or []
        if cache.get("window") is not window or not (
            len(blocks) == len(translations) == len(targets)
        ):
            window.editing_finished()
            self.notify(APP_TITLE, "当前结果没有完整的文本块数据，请重新翻译后再校对")
            return

        dialog = BlockEditorDialog(
            blocks,
            translations,
            targets,
            self.cfg.get("lang.zh_target", "en"),
            parent=window,
            target_modes=cache.get("target_overrides"),
        )
        dialog.blockRetranslateRequested.connect(
            self._weak_sender_slot(
                dialog,
                self._on_editor_retranslate_requested,
                weakref.ref(window),
            )
        )
        dialog.finished.connect(
            self._weak_sender_slot(
                dialog,
                self._on_editor_dialog_finished,
                weakref.ref(window),
            )
        )
        self._editor = dialog
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def _close_editor(self, window=None) -> None:
        dialog = self._editor
        if dialog is None:
            return
        if window is not None and dialog.parent() is not window:
            return
        self._editor = None
        dialog.reject()

    def _on_editor_retranslate_requested(
        self, dialog, window_ref, index: int, source: str, target
    ) -> None:
        window = window_ref()
        if window is not None:
            self._retranslate_blocks(window, [(index, source, target)], dialog)

    def _on_editor_dialog_finished(self, dialog, window_ref, result: int) -> None:
        window = window_ref()
        if window is not None:
            self._on_editor_finished(dialog, window, result)
        else:
            dialog.setParent(None)
            dialog.deleteLater()

    def _on_editor_finished(self, dialog, window, result: int) -> None:
        if self._editor is dialog:
            self._editor = None

        edits = []
        if result == int(QDialog.DialogCode.Accepted):
            edits = [
                (edit.index, edit.source_text, edit.target)
                for edit in dialog.changes()
            ]

        # QDialog.reject()/accept() only hides a dialog. Without explicitly
        # detaching and deleting it, every editor ever opened remains a child
        # of the result window together with all text widgets and connections.
        dialog.setParent(None)
        dialog.deleteLater()

        pending = self._pending
        same_dialog_subset = bool(
            pending
            and pending.get("mode") == "subset"
            and pending.get("window") is window
            and pending.get("dialog") is dialog
        )
        if same_dialog_subset:
            # 这个请求已持有 worker；先让它自然结束，再串行提交关闭窗口时留下的草稿。
            # 同时解除 dialog 引用，避免回调向已经关闭的窗口写错误或译文。
            pending["dialog"] = None
            if edits:
                pending["deferred_edits"] = edits
            return

        started = bool(edits) and self._retranslate_blocks(window, edits, None)

        pending_for_window = bool(
            self._pending
            and self._pending.get("mode") == "subset"
            and self._pending.get("window") is window
        )
        if not started and not pending_for_window:
            window.editing_finished()

    @staticmethod
    def _store_block_edit(block, source: str) -> None:
        """只在文字确实不同于 OCR 时保留覆盖，改回原文就清掉覆盖。"""
        source = source.strip()
        block.edited_text = None if source == block.recognized_text else source

    def _retranslate_blocks(self, window, edits, dialog=None) -> bool:
        """应用若干校对，并只把这些块交给翻译接口。"""
        cache = self._last
        blocks = cache.get("blocks") or []
        overrides = cache.get("target_overrides")
        if cache.get("window") is not window or not blocks:
            if dialog is not None:
                for index, _source, _target in edits:
                    dialog.set_error(index, "当前译文已经失效，请重新框选")
            return False
        if overrides is None or len(overrides) != len(blocks):
            overrides = [None] * len(blocks)
            cache["target_overrides"] = overrides

        pending_updates: dict[int, tuple[str, str | None]] = {}
        for index, source, target_mode in edits:
            if index < 0 or index >= len(blocks):
                self.notify(APP_TITLE, "一个文本块已经失效，请重新打开校对窗口")
                continue
            source = source.strip()
            if not source:
                if dialog is not None:
                    dialog.set_error(index, "识别原文不能为空")
                else:
                    self.notify(APP_TITLE, f"第 {index + 1} 个文本块为空，未应用")
                continue
            requested = None if target_mode in (None, "auto") else target_mode
            pending_updates[index] = (source, requested)

        indices = list(pending_updates)
        if not indices:
            return False
        return self._start_subset_worker(window, indices, dialog, pending_updates)

    def _start_subset_worker(
        self, window, indices: list[int], dialog=None, updates=None
    ) -> bool:
        from .worker import TranslateWorker

        cache = self._last
        image = cache.get("image")
        blocks = cache.get("blocks") or []
        overrides = cache.get("target_overrides") or [None] * len(blocks)
        if cache.get("window") is not window or not blocks:
            if dialog is not None:
                for index in indices:
                    dialog.set_error(index, "没有可重译的文本块")
            return False

        # 编辑器允许切到另一块继续提交；新请求会取代尚未完成的旧单块请求。
        old = self._pending
        if old and old.get("mode") == "subset":
            old_dialog = old.get("dialog")
            if old_dialog is not None:
                for index in old.get("indices", []):
                    old_dialog.set_error(index, "已由新的重译请求替换，请重试")
        self._request_serial += 1
        request_id = self._request_serial
        updates = dict(updates or {})
        subset = []
        requested_targets = []
        for index in indices:
            source, requested = updates.get(index, (blocks[index].text, overrides[index]))
            shadow = copy.copy(blocks[index])
            self._store_block_edit(shadow, source)
            subset.append(shadow)
            requested_targets.append(requested)
            updates[index] = (source, requested)
        request = {
            "mode": "subset",
            "id": request_id,
            "worker": None,
            "factory": TranslateWorker,
            "image": image,
            "window": window,
            "indices": list(indices),
            "dialog": dialog,
            "updates": updates,
            "worker_blocks": subset,
            "worker_targets": requested_targets,
        }
        return self._schedule_worker_request(request)

    def _on_result_closed(self, window):
        self._close_editor(window)
        if self.result is window:
            self.result = None
        if self.minimized_result is window:
            self._set_minimized(None)
        if self._last.get("window") is window:
            self._last = {}
        if self._pending and self._pending.get("window") is window:
            self._pending = None
            self._cancel_active_worker()
        self._capture_serial += 1

    def _on_translated(self, worker, request_id, blocks, translations, plain_text, targets):
        pending = self._pending
        if (not pending or pending.get("id") != request_id
                or pending.get("worker") is not worker or self.worker is not worker):
            return
        window = pending["window"]
        image = pending["image"]
        self._pending = None
        self.worker = None
        overrides = pending.get("target_overrides")
        if overrides is None or len(overrides) != len(blocks):
            overrides = [None] * len(blocks)
        # 存下来给「重新翻译」和逐块校对用。四个列表始终同序、同长度。
        self._last = {
            "image": image,
            "window": window,
            "blocks": blocks,
            "translations": list(translations),
            "targets": list(targets),
            "target_overrides": list(overrides),
        }
        self._render_cached_result(window, plain_text)

    def _on_subset_translated(
        self, worker, request_id, _blocks, translations, _plain_text, targets
    ) -> None:
        pending = self._pending
        if (
            not pending
            or pending.get("mode") != "subset"
            or pending.get("id") != request_id
            or pending.get("worker") is not worker
            or self.worker is not worker
        ):
            return
        window = pending["window"]
        indices = pending["indices"]
        dialog = pending.get("dialog")
        updates = pending.get("updates") or {}
        deferred_edits = list(pending.get("deferred_edits") or [])
        self._pending = None
        self.worker = None

        cache = self._last
        cached_translations = cache.get("translations") or []
        cached_targets = cache.get("targets") or []
        if (
            cache.get("window") is not window
            or len(translations) != len(indices)
            or len(targets) != len(indices)
            or len(cached_translations) != len(cache.get("blocks") or [])
            or len(cached_targets) != len(cache.get("blocks") or [])
        ):
            message = "单块重译结果与当前文本块不一致，请重新打开校对窗口"
            if dialog is not None and self._editor is dialog:
                for index in indices:
                    dialog.set_error(index, message)
            else:
                self.notify(APP_TITLE, message)
            if deferred_edits and self._retranslate_blocks(window, deferred_edits, None):
                return
            window.editing_finished()
            return

        for index, translation, target in zip(indices, translations, targets):
            source, requested = updates[index]
            self._store_block_edit(cache["blocks"][index], source)
            cache["target_overrides"][index] = requested
            cached_translations[index] = translation
            cached_targets[index] = target
            if dialog is not None and self._editor is dialog:
                dialog.set_translation(index, translation, target)

        self._render_cached_result(window)
        if deferred_edits and self._retranslate_blocks(window, deferred_edits, None):
            return
        if self._editor is None:
            window.editing_finished()

    def _on_subset_failed(self, worker, request_id, message: str) -> None:
        pending = self._pending
        if (
            not pending
            or pending.get("mode") != "subset"
            or pending.get("id") != request_id
            or pending.get("worker") is not worker
            or self.worker is not worker
        ):
            return
        window = pending["window"]
        dialog = pending.get("dialog")
        indices = pending.get("indices", [])
        deferred_edits = list(pending.get("deferred_edits") or [])
        self._pending = None
        self.worker = None
        if dialog is not None and self._editor is dialog:
            for index in indices:
                dialog.set_error(index, message)
        else:
            self.notify(f"{APP_TITLE} · 单块重译失败", message)
        if deferred_edits and self._retranslate_blocks(window, deferred_edits, None):
            return
        if self._editor is None:
            window.editing_finished()

    def _render_cached_result(self, window, plain_text: str | None = None) -> bool:
        from . import render

        cache = self._last
        if cache.get("window") is not window:
            return False
        image = cache.get("image")
        blocks = cache.get("blocks") or []
        translations = cache.get("translations") or []
        if len(blocks) != len(translations):
            window.set_error("缓存的文本块与译文数量不一致，请重新框选")
            return False
        if image is None:
            image = render.qimage_to_pil(window.original_image())
        try:
            canvas, layouts = render.render(
                image,
                blocks,
                translations,
                font_family=self.cfg.get("appearance.font_family", "Microsoft YaHei UI"),
                min_font_px=int(self.cfg.get("appearance.min_font_px", 9)),
            )
        except Exception as exc:
            report_exception(logging.getLogger("screentrans.errors"), "render.result", exc)
            window.set_error(f"排版失败：{exc}")
            return False

        if plain_text is None:
            plain_text = "\n".join(text for text in translations if text)
        window.set_result(canvas, plain_text, layouts)
        # The result window already owns the original pixels. Keeping another
        # PIL copy here costs ~24 MiB for a 4K selection; retries with existing
        # OCR blocks do not read it, and re-rendering restores it on demand.
        cache["image"] = None
        if self.cfg.get("appearance.auto_copy", True) and plain_text:
            self.qapp.clipboard().setText(plain_text)
        return True

    def _on_failed(self, worker, request_id, message: str):
        pending = self._pending
        if (not pending or pending.get("id") != request_id
                or pending.get("worker") is not worker or self.worker is not worker):
            return
        window = pending["window"]
        self._pending = None
        self.worker = None
        window.set_error(message)

    @staticmethod
    def _logical_rect(screen, phys_rect) -> QRect:
        """物理像素矩形 -> Qt 逻辑坐标矩形，用来把结果窗口放回原位。"""
        pl, pt, _pw, _ph = winsys.screen_physical_rect(screen)
        dpr = screen.devicePixelRatio() or 1.0
        geo = screen.geometry()
        left, top, width, height = phys_rect
        return QRect(
            geo.left() + int(round((left - pl) / dpr)),
            geo.top() + int(round((top - pt) / dpr)),
            max(1, int(round(width / dpr))),
            max(1, int(round(height / dpr))),
        )

    @staticmethod
    def _physical_rect(screen, logical: QRect) -> tuple[int, int, int, int]:
        """`_logical_rect` 的反向：Qt 逻辑矩形 -> 物理像素的虚拟桌面矩形。"""
        pl, pt, pw, ph = winsys.screen_physical_rect(screen)
        dpr = screen.devicePixelRatio() or 1.0
        geo = screen.geometry()
        left = pl + int(round((logical.left() - geo.left()) * dpr))
        top = pt + int(round((logical.top() - geo.top()) * dpr))
        width = max(1, int(round(logical.width() * dpr)))
        height = max(1, int(round(logical.height() * dpr)))
        # 夹回本屏范围，别越到隔壁显示器上去
        left = max(pl, min(left, pl + pw - 1))
        top = max(pt, min(top, pt + ph - 1))
        return left, top, min(width, pl + pw - left), min(height, pt + ph - top)

    # ------------------------------------------------------ 拖边框 = 重新框范围
    def _on_region_changed(self, window):
        """用户把译文窗口拖大/拖小了：重新截这块屏幕，重新认字、重新翻。

        以前拖动只是把那张图拉伸，字会变形，而且多框进来的部分根本没被翻译。
        现在拖动的含义变成「我要框的其实是这一块」。
        """
        if window is not self.result or not window.isVisible():
            return
        self._close_editor(window)
        self._cancel_active_worker()
        self._pending = None
        self._last = {}
        self._capture_serial += 1
        capture_id = self._capture_serial
        rect = QRect(window.geometry())
        screen = (self.qapp.screenAt(rect.center()) or window.screen()
                  or self.qapp.primaryScreen())
        # 一次截图只能使用一块屏幕的一套 DPR；窗口边缘已在 ResultWindow 里限制，
        # 这里再夹一次，挡住程序化 setGeometry 等绕过鼠标约束的调用。
        rect = rect.intersected(screen.geometry())
        if rect.isEmpty():
            window.set_error("重新截屏失败：选区不在任何显示器内")
            return
        if rect != window.geometry():
            window.setGeometry(rect)
        window.begin_recapture()
        # 让「把自己变透明」这一帧真的合成上屏，再截。
        # 不等的话截到的就是自己刚画上去的译文，越拖越糊，套娃。
        QTimer.singleShot(
            80,
            lambda w=window, s=screen, r=QRect(rect), cid=capture_id:
            self._grab_region(w, s, r, cid),
        )

    def _grab_region(self, window, screen, rect: QRect, capture_id: int):
        from . import capture, render

        # 较早的定时器、已经关闭的窗口一律不碰。尤其不能在这里调用旧窗口的
        # end_recapture：WA_DeleteOnClose 下它的 C++ 对象可能已经不存在了。
        if capture_id != self._capture_serial or window is not self.result:
            return
        try:
            crop = capture.grab(*self._physical_rect(screen, rect))
        except Exception as exc:
            report_exception(logging.getLogger("screentrans.errors"), "capture.recapture", exc)
            window.end_recapture(None)
            window.set_error(f"重新截屏失败：{exc}")
            return
        window.end_recapture(render.pil_to_qimage(crop), rect, screen)
        self._start_worker(crop, window)

    # ---------------------------------------------------------------- 设置
    def open_settings(self):
        if self.settings is None:
            from .ui.settings_window import SettingsWindow

            self.settings = SettingsWindow(self.cfg)
            self.settings.setWindowIcon(self.icon)
            self.settings.hotkeyEdited.connect(self._apply_hotkey)
            self.settings.restartRequested.connect(self.restart)
            self.settings.settingsSaved.connect(self._apply_accent)
            # 快捷键是启动时就注册好的，那会儿设置窗口还不存在，
            # 把当时的结果补进去，免得刚打开时状态栏一片空白
            for name, (ok, message) in self._hotkey_state.items():
                self.settings.set_hotkey_status(name, ok, message)
        self.settings.show_front()

    # -------------------------------------------------------------- 软件更新
    def _ensure_update_dialog(self) -> UpdateDialog:
        if self.updates is None:
            from .ui.update_dialog import UpdateDialog

            self.updates = UpdateDialog(self.cfg)
            self.updates.setWindowIcon(self.icon)
            self.updates.updateAvailable.connect(self._on_update_available)
            self.updates.installRequested.connect(self._on_install_requested)
        return self.updates

    def open_updates(self) -> None:
        dialog = self._ensure_update_dialog()
        dialog.show_front()
        if str(self.cfg.get("updates.manifest_url", "") or "").strip():
            dialog.check()

    def _check_updates_silently(self) -> None:
        if not str(self.cfg.get("updates.manifest_url", "") or "").strip():
            return
        self._ensure_update_dialog().check_silently()

    def _on_update_available(self, info) -> None:
        self.act_update.setText(f"发现新版本 {info.version}…")
        self.notify(
            f"{APP_TITLE} · 有新版本",
            f"ScreenTranslate {info.version} 已发布，可从检查更新窗口直接下载并安装。",
        )

    def _on_install_requested(self, path, info) -> None:
        from .updater import UpdateError, install_helper_arguments

        repository_url = str(self.cfg.get("updates.repository_url", "") or "").strip()
        try:
            arguments = install_helper_arguments(
                path,
                info,
                repository_url=repository_url,
                parent_pid=os.getpid(),
            )
            winsys.relaunch(arguments)
        except Exception as exc:
            report_exception(
                logging.getLogger("screentrans.errors"),
                "update.start_installer_helper",
                exc,
            )
            message = (
                str(exc)
                if isinstance(exc, UpdateError)
                else f"无法启动安装程序（{type(exc).__name__}）"
            )
            if self.updates is not None:
                self.updates.install_failed(message)
            return
        self.quit()

    def _apply_accent(self):
        """强调色改了就地换掉，不用重启。

        托盘图标、托盘菜单的高亮、下一次框选的边框都跟着变。
        已经开着的译文窗口不动——它的颜色是建窗口那一刻定的，
        中途改掉等于在用户眼皮底下变色，比"下一个窗口才变"更奇怪。
        """
        accent = self.cfg.get("appearance.accent", "#28C76F")
        if accent == self.accent:
            return
        self.accent = accent
        self.icon = make_icon(accent)
        self.tray.setIcon(self.icon)
        self.overlays.set_accent(accent)
        menu = self.tray.contextMenu()
        if menu is not None:
            menu.setStyleSheet(build_qss(accent))
        if self.settings is not None:
            self.settings.setWindowIcon(self.icon)
        if self.updates is not None:
            self.updates.setWindowIcon(self.icon)
            self.updates.setStyleSheet(build_qss(accent))

    def restart(self):
        """重启自己。切 OCR 引擎、装了新语言包之类的改动只能靠重启生效。"""
        self._shutdown()
        # 新进程带上 --restarted：它会等这边的互斥体释放，而不是直接报「已经在运行」
        winsys.relaunch(["--restarted"])
        self.qapp.quit()

    def quit(self):
        self._shutdown()
        self.qapp.quit()

    def _shutdown(self):
        """退出前的收尾：撤快捷键、收托盘图标、关掉还开着的译文窗口。"""
        if self._shutting_down:
            return
        self._shutting_down = True
        self._capture_serial += 1
        self.overlays.close()
        self._close_editor()
        self._cancel_active_worker()
        for worker in self._workers:
            self._cancel_worker(worker)
        self.hotkey_host.unregister()
        from . import capture

        capture.close()
        for window in (self.result, self.minimized_result):
            if window is not None:
                window.close()
        self.result = None
        self.minimized_result = None
        if self.settings is not None:
            self.settings.shutdown()
            self.settings.close()
            self.settings = None
        if self.updates is not None:
            self.updates.shutdown()
            self.updates.close()
            self.updates = None
        self.tray.hide()
        # 后台任务是 daemon thread；这里只给资源正常关闭一个固定总时间，网络异常
        # 不再无限卡住应用退出。
        deadline = time.monotonic() + WORKER_SHUTDOWN_WAIT_MS / 1000
        for worker in list(self._workers):
            remaining = max(0, int((deadline - time.monotonic()) * 1000))
            worker.wait(remaining)
        self._workers.clear()


def selftest() -> int:
    """`ScreenTranslate.exe --selftest`：逐项检查 OCR、排版、翻译接口、快捷键。

    打包成 exe 之后 OCR 组件最容易失效（WinRT 的 .pyd 是动态加载的），
    出问题时跑一下这个就知道断在哪一环。
    """
    from PIL import Image, ImageDraw, ImageFont

    from . import render
    from .network import configured_secrets, redact_sensitive

    lines_out: list[str] = []

    def log(ok, name, detail=""):
        mark = "[OK]  " if ok else "[FAIL]"
        lines_out.append(f"{mark} {name}" + (f"\n       {detail}" if detail else ""))

    def safe_error(exc: Exception, secrets=()) -> str:
        return f"{type(exc).__name__}: {redact_sensitive(exc, secrets)}"

    qapp = QApplication(sys.argv)
    cfg = Config()
    offline = "--offline" in sys.argv
    if offline and cfg.get("ocr.engine", "windows") in {"youdao_cloud", "azure_vision"}:
        # 离线自检不能沿用用户选中的云端 OCR；只改内存，不写回配置。
        cfg.set("ocr.engine", "windows")

    # 1) OCR 引擎
    try:
        from .ocr import windows_ocr

        langs = windows_ocr.available_languages()
        log(bool(langs), "系统 OCR 语言包", "、".join(langs) if langs else "一个都没装")
    except Exception as exc:
        log(False, "系统 OCR 语言包", f"{type(exc).__name__}: {exc}")
        langs = []

    # 2) 真正识别一次
    img = Image.new("RGB", (460, 100), (255, 255, 255))
    d = ImageDraw.Draw(img)
    font = None
    for name in ("msyh.ttc", "simhei.ttf", "simsun.ttc"):
        try:
            font = ImageFont.truetype(name, 20)
            break
        except OSError:
            continue
    d.text((16, 14), "Hello world, testing OCR.", font=font, fill=(20, 20, 20))
    d.text((16, 52), "中文识别测试。", font=font, fill=(20, 20, 20))
    blocks, texts = [], []
    try:
        from . import ocr as ocr_mod
        from .layout import group_lines

        recognized = ocr_mod.recognize(img, cfg)
        blocks = group_lines(recognized)
        texts = [b.text for b in blocks]
        log(bool(texts), "文字识别", " | ".join(texts) if texts else "没识别到任何文字")
    except Exception as exc:
        ocr_opts = cfg.get("ocr.azure_vision", {}) or {}
        log(False, "文字识别", safe_error(exc, configured_secrets(ocr_opts)))

    # 3) 排版渲染
    try:
        render.render(img, blocks, ["测试" for _ in blocks],
                      cfg.get("appearance.font_family", "Microsoft YaHei UI"), 9)
        log(True, "译文排版渲染")
    except Exception as exc:
        log(False, "译文排版渲染", f"{type(exc).__name__}: {exc}")

    # 4) 翻译接口
    provider = cfg.get("translator.provider", "microsoft")
    opts = cfg.get(f"translator.{provider}", {}) or {}
    engine = None
    try:
        from . import translators

        engine = translators.build(provider, opts)
        if offline:
            log(True, f"翻译模块（{provider}）", "离线模式：已成功装载，不发送网络请求")
        else:
            log(True, f"翻译接口（{provider}）", engine.check())
    except Exception as exc:
        log(
            False,
            f"翻译接口（{provider}）",
            safe_error(exc, configured_secrets(opts)),
        )
    finally:
        if engine is not None:
            close = getattr(engine, "close", None)
            if callable(close):
                try:
                    close()
                except Exception:
                    pass

    # 5) 快捷键
    try:
        host = HotkeyHost()
        for name, key, default, label, _desc in HOTKEYS:
            spec = cfg.get(key, default)
            ok, msg = host.register(name, spec)
            log(ok, f"全局快捷键 · {label}（{spec}）", msg)
        host.unregister()
    except Exception as exc:
        log(False, "全局快捷键", f"{type(exc).__name__}: {exc}")

    from .config import CONFIG_DIR, CONFIG_PATH

    failed = sum(1 for l in lines_out if l.startswith("[FAIL]"))
    report = (
        f"{APP_TITLE} 自检报告\n配置文件：{CONFIG_PATH}\n\n"
        + "\n".join(lines_out)
        + f"\n\n{'全部通过' if not failed else str(failed) + ' 项未通过'}"
    )

    try:
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        (CONFIG_DIR / "selftest.txt").write_text(report, encoding="utf-8")
    except Exception:
        pass

    print(report)
    if "--quiet" not in sys.argv:
        ctypes.windll.user32.MessageBoxW(0, report, f"{APP_TITLE} 自检", 0x40)
    del qapp
    return 1 if failed else 0


def main() -> int:
    if "--apply-update" in sys.argv[1:]:
        from .updater import UpdateError, run_install_helper

        try:
            run_install_helper(sys.argv)
            return 0
        except Exception as exc:
            report_exception(
                logging.getLogger("screentrans.errors"),
                "update.install_helper",
                exc,
            )
            message = (
                str(exc)
                if isinstance(exc, UpdateError)
                else f"无法启动安装程序（{type(exc).__name__}）"
            )
            ctypes.windll.user32.MessageBoxW(
                0,
                message,
                f"{APP_TITLE} 更新失败",
                0x10,
            )
            return 1

    if "--selftest" in sys.argv:
        return selftest()

    # 「重启」拉起来的新进程，要给上一个进程几秒钟退干净
    if _already_running(5.0 if "--restarted" in sys.argv else 0.0):
        ctypes.windll.user32.MessageBoxW(
            0, f"{APP_TITLE} 已经在运行了，请看右下角托盘图标。", APP_TITLE, 0x40
        )
        return 0

    QApplication.setHighDpiScaleFactorRoundingPolicy(
        Qt.HighDpiScaleFactorRoundingPolicy.PassThrough
    )
    qapp = QApplication(sys.argv)
    qapp.setApplicationName(APP_TITLE)
    qapp.setQuitOnLastWindowClosed(False)

    if not QSystemTrayIcon.isSystemTrayAvailable():
        ctypes.windll.user32.MessageBoxW(0, "系统托盘不可用，程序无法常驻。", APP_TITLE, 0x10)
        return 1

    app = App(qapp)
    app.notify(APP_TITLE, f"已启动，按 {app.cfg.get('hotkey', 'Ctrl+Alt+Q')} 开始框选翻译")
    return qapp.exec()


if __name__ == "__main__":
    sys.exit(main())
