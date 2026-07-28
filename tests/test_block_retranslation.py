from __future__ import annotations

import os
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PIL import Image
from PySide6.QtWidgets import QDialog

from qt_helpers import MemoryConfig, application
from screentrans import config as config_module
from screentrans.layout import Block
from screentrans.ocr.base import Line


with patch.object(
    config_module,
    "CONFIG_PATH",
    Path(__file__).with_name("__missing_config__.json"),
):
    from screentrans.main import App


def _block(text: str, y: float) -> Block:
    return Block([Line(text, 0, y, 120, 20)])


class _Signal:
    def __init__(self):
        self.callbacks = []

    def connect(self, callback):
        self.callbacks.append(callback)


class _FakeWorker:
    instances = []

    def __init__(self, image, cfg, blocks, targets):
        self.image = image
        self.cfg = cfg
        self.blocks = blocks
        self.targets = targets
        self.done = _Signal()
        self.failed = _Signal()
        self.finished = _Signal()
        self.cancelled = False
        self.started = False
        self.__class__.instances.append(self)

    def start(self):
        self.started = True

    def deleteLater(self):
        pass


class BlockRetranslationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.qapp = application()

    def setUp(self):
        self.app = App.__new__(App)
        self.app.cfg = MemoryConfig({"appearance": {"auto_copy": False}})
        self.app.qapp = self.qapp
        self.app._request_serial = 4
        self.app._workers = []
        self.app._pending = None
        self.app.worker = None
        self.app._editor = None
        self.app.notify = Mock()
        self.blocks = [_block("第一块", 0), _block("Second block", 30)]
        self.image = Image.new("RGB", (160, 70), "white")
        self.window = Mock()
        self.app._last = {
            "image": self.image,
            "window": self.window,
            "blocks": self.blocks,
            "translations": ["First", "第二块"],
            "targets": ["en", "zh-Hans"],
            "target_overrides": [None, "en"],
        }

    def test_starting_single_block_worker_sends_only_that_block(self):
        _FakeWorker.instances.clear()
        dialog = Mock()

        with patch("screentrans.worker.TranslateWorker", _FakeWorker):
            started = self.app._start_subset_worker(self.window, [1], dialog)

        self.assertTrue(started)
        self.assertEqual(len(_FakeWorker.instances), 1)
        worker = _FakeWorker.instances[0]
        self.assertEqual(worker.blocks, [self.blocks[1]])
        self.assertIsNot(worker.blocks[0], self.blocks[1])
        self.assertEqual(worker.targets, ["en"])
        self.assertTrue(worker.started)
        self.assertEqual(self.app._last["translations"], ["First", "第二块"])

    def test_single_block_result_replaces_one_slot_and_renders_full_cache(self):
        worker = object()
        dialog = Mock()
        self.app._editor = dialog
        self.app.worker = worker
        self.app._pending = {
            "mode": "subset",
            "id": 9,
            "worker": worker,
            "window": self.window,
            "indices": [1],
            "dialog": dialog,
            "updates": {1: ("Second corrected", "ja")},
        }

        with patch("screentrans.render.render", return_value=("canvas", ["layout"])) as draw:
            self.app._on_subset_translated(
                worker,
                9,
                [self.blocks[1]],
                ["新的第二块"],
                "新的第二块",
                ["ja"],
            )

        self.assertEqual(self.app._last["translations"], ["First", "新的第二块"])
        self.assertEqual(self.app._last["targets"], ["en", "ja"])
        self.assertEqual(self.blocks[1].text, "Second corrected")
        self.assertEqual(self.app._last["target_overrides"], [None, "ja"])
        draw.assert_called_once()
        self.assertEqual(draw.call_args.args[:3], (self.image, self.blocks, ["First", "新的第二块"]))
        dialog.set_translation.assert_called_once_with(1, "新的第二块", "ja")
        self.window.set_result.assert_called_once_with("canvas", "First\n新的第二块", ["layout"])
        self.window.editing_finished.assert_not_called()

    def test_failed_block_request_does_not_commit_its_source_draft(self):
        worker = object()
        dialog = Mock()
        self.app._editor = dialog
        self.app.worker = worker
        self.app._pending = {
            "mode": "subset",
            "id": 10,
            "worker": worker,
            "window": self.window,
            "indices": [1],
            "dialog": dialog,
            "updates": {1: ("Uncommitted correction", "ja")},
        }

        self.app._on_subset_failed(worker, 10, "network failed")

        self.assertEqual(self.blocks[1].text, "Second block")
        self.assertEqual(self.app._last["target_overrides"], [None, "en"])
        dialog.set_error.assert_called_once_with(1, "network failed")

    def test_editor_does_not_reopen_over_pending_subset_request(self):
        worker = object()
        self.app.worker = worker
        self.app._pending = {
            "mode": "subset",
            "id": 11,
            "worker": worker,
            "window": self.window,
            "indices": [1],
            "dialog": None,
        }

        with patch("screentrans.ui.block_editor.BlockEditorDialog") as editor_type:
            self.app._open_block_editor(self.window)

        editor_type.assert_not_called()
        self.window.editing_finished.assert_called_once()
        self.app.notify.assert_called_once_with(
            "划词截屏翻译", "文本块仍在重译，请稍后再打开校对窗口"
        )
        self.assertIs(self.app._pending["worker"], worker)

    def test_accepting_editor_defers_edits_without_cancelling_its_subset(self):
        worker = SimpleNamespace(cancelled=False)
        dialog = Mock()
        dialog.changes.return_value = [
            SimpleNamespace(index=0, source_text="第一块修正", target="en")
        ]
        self.app._editor = dialog
        self.app.worker = worker
        self.app._pending = {
            "mode": "subset",
            "id": 12,
            "worker": worker,
            "window": self.window,
            "indices": [1],
            "dialog": dialog,
        }

        with patch.object(self.app, "_retranslate_blocks") as retranslate:
            self.app._on_editor_finished(
                dialog, self.window, int(QDialog.DialogCode.Accepted)
            )

        retranslate.assert_not_called()
        self.assertFalse(worker.cancelled)
        self.assertIsNone(self.app._editor)
        self.assertIsNone(self.app._pending["dialog"])
        self.assertEqual(
            self.app._pending["deferred_edits"], [(0, "第一块修正", "en")]
        )
        self.window.editing_finished.assert_not_called()

    def test_subset_success_starts_deferred_edits_after_clearing_pending(self):
        worker = object()
        deferred = [(0, "第一块修正", "en")]
        self.app.worker = worker
        self.app._pending = {
            "mode": "subset",
            "id": 13,
            "worker": worker,
            "window": self.window,
            "indices": [1],
            "dialog": None,
            "updates": {1: ("Second corrected", "ja")},
            "deferred_edits": deferred,
        }

        def start_deferred(window, edits, dialog):
            self.assertIsNone(self.app._pending)
            self.assertIsNone(self.app.worker)
            self.assertIs(window, self.window)
            self.assertEqual(edits, deferred)
            self.assertIsNone(dialog)
            return True

        with (
            patch.object(self.app, "_render_cached_result", return_value=True),
            patch.object(
                self.app, "_retranslate_blocks", side_effect=start_deferred
            ) as retranslate,
        ):
            self.app._on_subset_translated(
                worker,
                13,
                [self.blocks[1]],
                ["新的第二块"],
                "新的第二块",
                ["ja"],
            )

        retranslate.assert_called_once()
        self.window.editing_finished.assert_not_called()

    def test_closed_editor_failure_notifies_and_then_starts_deferred_edits(self):
        worker = object()
        closed_dialog = Mock()
        deferred = [(0, "第一块修正", "en")]
        self.app.worker = worker
        self.app._pending = {
            "mode": "subset",
            "id": 14,
            "worker": worker,
            "window": self.window,
            "indices": [1],
            "dialog": closed_dialog,
            "deferred_edits": deferred,
        }

        def start_deferred(window, edits, dialog):
            self.assertIsNone(self.app._pending)
            self.assertIsNone(self.app.worker)
            return True

        with patch.object(
            self.app, "_retranslate_blocks", side_effect=start_deferred
        ) as retranslate:
            self.app._on_subset_failed(worker, 14, "network failed")

        closed_dialog.set_error.assert_not_called()
        self.app.notify.assert_called_once_with(
            "划词截屏翻译 · 单块重译失败", "network failed"
        )
        retranslate.assert_called_once_with(self.window, deferred, None)
        self.window.editing_finished.assert_not_called()

    def test_restore_result_uses_result_window_restore_hook(self):
        self.app.minimized_result = self.window
        self.app.result = None
        self.app._set_minimized = Mock()

        self.app.restore_result()

        self.app._set_minimized.assert_called_once_with(None)
        self.assertIs(self.app.result, self.window)
        self.window.restore_from_tray.assert_called_once_with()

    def test_full_result_caches_parallel_translation_metadata(self):
        worker = object()
        self.app.worker = worker
        self.app._pending = {
            "mode": "full",
            "id": 12,
            "worker": worker,
            "window": self.window,
            "image": self.image,
            "target_overrides": [None, "en"],
        }

        with patch.object(self.app, "_render_cached_result", return_value=True):
            self.app._on_translated(
                worker,
                12,
                self.blocks,
                ["First", "Second"],
                "First\nSecond",
                ["en", "en"],
            )

        self.assertEqual(self.app._last["translations"], ["First", "Second"])
        self.assertEqual(self.app._last["targets"], ["en", "en"])
        self.assertEqual(self.app._last["target_overrides"], [None, "en"])


if __name__ == "__main__":
    unittest.main()
