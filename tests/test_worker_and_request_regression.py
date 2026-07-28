from __future__ import annotations

import os
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from qt_helpers import MemoryConfig, application
from screentrans import config as config_module
from screentrans.translators.base import TranslateError, Translator
from screentrans.worker import TranslateWorker, translate_blocks


# main preloads the configured OCR engine at import time. Keep discovery isolated
# from the developer machine's real config and optional ONNX installation.
with patch.object(
    config_module,
    "CONFIG_PATH",
    Path(__file__).with_name("__missing_config__.json"),
):
    from screentrans.main import App


class WorkerContractRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = application()

    def test_provider_segment_count_mismatch_fails_instead_of_padding(self):
        cfg = MemoryConfig(
            {
                "lang": {"zh_target": "en"},
                "translator": {"provider": "fake", "fake": {}},
            }
        )
        worker = TranslateWorker(None, cfg, [SimpleNamespace(text="\u6d4b\u8bd5")])
        completed = []
        failures = []
        worker.done.connect(lambda *args: completed.append(args))
        worker.failed.connect(failures.append)
        engine = Mock()
        engine.translate.return_value = []

        with patch("screentrans.worker.translators.build", return_value=engine):
            worker.run()

        self.assertEqual(completed, [])
        self.assertEqual(len(failures), 1)

    def test_mixed_language_blocks_are_grouped_and_restored_in_screen_order(self):
        cfg = MemoryConfig(
            {
                "lang": {"zh_target": "en"},
                "translator": {"provider": "fake", "fake": {}},
            }
        )
        blocks = [
            SimpleNamespace(text="这是中文段落"),
            SimpleNamespace(text="This is an English paragraph."),
            SimpleNamespace(text="配置 PyInstallerBuildMode"),
            SimpleNamespace(text="Open the settings panel."),
        ]
        worker = TranslateWorker(None, cfg, blocks)
        completed = []
        calls = []
        worker.done.connect(lambda *args: completed.append(args))
        engine = Mock()

        def translate(texts, target, source):
            calls.append((list(texts), target, source))
            prefix = "EN:" if target == "en" else "ZH:"
            return [prefix + text for text in texts]

        engine.translate.side_effect = translate
        with patch("screentrans.worker.translators.build", return_value=engine):
            worker.run()

        self.assertEqual(
            calls,
            [
                (["这是中文段落", "配置 PyInstallerBuildMode"], "en", None),
                (["This is an English paragraph.", "Open the settings panel."], "zh-Hans", None),
            ],
        )
        self.assertEqual(len(completed), 1)
        _blocks, translated, plain, targets = completed[0]
        self.assertEqual(
            translated,
            [
                "EN:这是中文段落",
                "ZH:This is an English paragraph.",
                "EN:配置 PyInstallerBuildMode",
                "ZH:Open the settings panel.",
            ],
        )
        self.assertEqual(targets, ["en", "zh-Hans", "en", "zh-Hans"])
        self.assertEqual(plain, "\n".join(translated))

    def test_grouped_translation_can_be_cancelled_before_next_target(self):
        blocks = [
            SimpleNamespace(text="中文内容"),
            SimpleNamespace(text="English content"),
        ]
        cancelled = False
        calls = []
        engine = Mock()

        def translate(texts, target, source):
            nonlocal cancelled
            calls.append((list(texts), target, source))
            cancelled = True
            return ["translated"] * len(texts)

        engine.translate.side_effect = translate
        with self.assertRaisesRegex(TranslateError, "已取消"):
            translate_blocks(
                engine,
                blocks,
                cancel_check=lambda: cancelled,
            )
        self.assertEqual(calls, [(["中文内容"], "en", None)])

    def test_explicit_target_can_be_mixed_with_automatic_targets(self):
        blocks = [
            SimpleNamespace(text="中文内容"),
            SimpleNamespace(text="English content"),
        ]
        engine = Mock()
        engine.translate.side_effect = lambda texts, target, _source: [
            f"{target}:{text}" for text in texts
        ]

        translated, targets = translate_blocks(
            engine,
            blocks,
            targets=["ja", None],
        )

        self.assertEqual(targets, ["ja", "zh-Hans"])
        self.assertEqual(
            translated,
            ["ja:中文内容", "zh-Hans:English content"],
        )

    def test_translator_base_rejects_segment_count_mismatch(self):
        class ShortTranslator(Translator):
            def _translate_batch(self, texts, target, source):
                return []

        with self.assertRaises(TranslateError):
            ShortTranslator({}).translate(["one segment"], "en")

    def test_cancellation_stops_before_another_batch(self):
        calls = []

        class OneAtATimeTranslator(Translator):
            max_items = 1

            def _translate_batch(self, texts, target, source):
                calls.append(list(texts))
                return list(texts)

        engine = OneAtATimeTranslator({})
        engine.cancel_check = lambda: len(calls) >= 1
        with self.assertRaises(TranslateError):
            engine.translate(["first", "second"], "en")
        self.assertEqual(calls, [["first"]])


class StaleRequestRegressionTests(unittest.TestCase):
    def setUp(self):
        self.app = App.__new__(App)
        self.active_worker = object()
        self.window = Mock()
        self.pending = {
            "id": 11,
            "worker": self.active_worker,
            "window": self.window,
            "image": object(),
            "blocks": None,
        }
        self.app._pending = self.pending
        self.app.worker = self.active_worker

    def test_translated_signal_from_old_request_id_is_discarded(self):
        with patch("screentrans.render.render") as render_call:
            App._on_translated(
                self.app,
                self.active_worker,
                10,
                [],
                [],
                "stale",
                "en",
            )
        render_call.assert_not_called()
        self.window.set_result.assert_not_called()
        self.assertIs(self.app._pending, self.pending)

    def test_translated_and_failed_signals_from_old_worker_are_discarded(self):
        old_worker = object()
        with patch("screentrans.render.render") as render_call:
            App._on_translated(
                self.app,
                old_worker,
                11,
                [],
                [],
                "stale",
                "en",
            )
            App._on_failed(self.app, old_worker, 11, "stale failure")
        render_call.assert_not_called()
        self.window.set_result.assert_not_called()
        self.window.set_error.assert_not_called()
        self.assertIs(self.app._pending, self.pending)


class _ManualSignal:
    def __init__(self):
        self.callbacks = []

    def connect(self, callback):
        self.callbacks.append(callback)


class _ManualWorker:
    instances = []

    def __init__(self, image, cfg, blocks, targets):
        self.image = image
        self.done = _ManualSignal()
        self.failed = _ManualSignal()
        self.finished = _ManualSignal()
        self.cancelled = False
        self.running = False
        self.deleted = False
        self.__class__.instances.append(self)

    def start(self):
        self.running = True

    def cancel(self):
        self.cancelled = True

    def isRunning(self):
        return self.running

    def deleteLater(self):
        self.deleted = True


class WorkerConcurrencyRegressionTests(unittest.TestCase):
    def setUp(self):
        self.app = App.__new__(App)
        self.app.cfg = MemoryConfig()
        self.app._shutting_down = False
        self.app._request_serial = 0
        self.app._workers = []
        self.app._queued_worker = None
        self.app._pending = None
        self.app.worker = None
        self.app._last = {}
        self.window = object()
        _ManualWorker.instances.clear()

    def test_rapid_requests_cap_running_workers_and_keep_only_latest_queue(self):
        with patch("screentrans.worker.TranslateWorker", _ManualWorker):
            self.app._start_worker("image-1", self.window)
            self.app._start_worker("image-2", self.window)
            self.app._start_worker("image-3", self.window)
            self.app._start_worker("image-4", self.window)

        self.assertEqual(len(_ManualWorker.instances), 2)
        self.assertTrue(all(worker.cancelled for worker in _ManualWorker.instances))
        self.assertEqual(self.app._queued_worker["image"], "image-4")
        self.assertIs(self.app._pending, self.app._queued_worker)

        oldest = _ManualWorker.instances[0]
        oldest.running = False
        oldest.finished.callbacks[0]()

        self.assertEqual(len(_ManualWorker.instances), 3)
        self.assertEqual(_ManualWorker.instances[-1].image, "image-4")
        self.assertTrue(_ManualWorker.instances[-1].running)
        self.assertIsNone(self.app._queued_worker)

if __name__ == "__main__":
    unittest.main()
