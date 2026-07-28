from __future__ import annotations

import gc
import os
import time
import unittest
import weakref
from pathlib import Path
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PIL import Image, ImageChops, ImageDraw, ImageFilter
from PySide6.QtCore import QCoreApplication, QEvent, QRect
from PySide6.QtGui import QColor

from qt_helpers import MemoryConfig, application, result_fixture
from screentrans import capture, config as config_module, render
from screentrans.layout import Block
from screentrans.ocr.base import Line
from screentrans.ui.block_editor import BlockEditorDialog
from screentrans.worker import TranslateWorker


with patch.object(
    config_module,
    "CONFIG_PATH",
    Path(__file__).with_name("__missing_config__.json"),
):
    from screentrans.main import App, WORKER_SHUTDOWN_WAIT_MS


def _colors(image: Image.Image, rect: QRect, line_height: float):
    pixels = render._RGBPixels(
        image.width, image.height, image.tobytes("raw", "RGB"), image
    )
    background, foreground = render.sample_colors(pixels, rect, line_height)
    return (
        (background.red(), background.green(), background.blue()),
        (foreground.red(), foreground.green(), foreground.blue()),
    )


class RenderResourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.qapp = application()

    def test_color_sampling_preserves_flat_and_gradient_results(self):
        flat = Image.new("RGB", (80, 50), (240, 242, 245))
        draw = ImageDraw.Draw(flat)
        draw.rectangle((18, 14, 60, 32), fill=(25, 28, 34))
        self.assertEqual(
            _colors(flat, QRect(15, 10, 50, 28), 18),
            ((240, 242, 245), (25, 28, 34)),
        )

        gradient = Image.new("RGB", (64, 40))
        pixels = gradient.load()
        for y in range(40):
            for x in range(64):
                pixels[x, y] = (20 + x * 2, 40 + y * 3, (x * 3 + y * 2) % 256)
        ImageDraw.Draw(gradient).text((12, 10), "ABC", fill=(250, 250, 245))
        self.assertEqual(
            _colors(gradient, QRect(10, 8, 36, 18), 16),
            ((75, 90, 116), (205, 211, 209)),
        )

    def test_qimage_round_trip_is_pixel_exact_with_padded_rows(self):
        source = Image.new("RGB", (3, 2))
        source.putdata(
            [(1, 2, 3), (4, 5, 6), (7, 8, 9), (10, 11, 12), (13, 14, 15), (16, 17, 18)]
        )
        restored = render.qimage_to_pil(render.pil_to_qimage(source))
        self.assertEqual(restored.size, source.size)
        self.assertEqual(restored.tobytes(), source.tobytes())

    def test_4k_color_sampling_stays_out_of_python_pixel_loops(self):
        image = Image.new("RGB", (3840, 2160), (238, 240, 244))
        started = time.perf_counter()
        colors = _colors(image, QRect(0, 0, 3840, 2160), 48)
        elapsed = time.perf_counter() - started
        self.assertEqual(colors, ((238, 240, 244), (0, 0, 0)))
        # The removed byte-by-byte path takes roughly 6-10 seconds on this
        # case; the C-backed Pillow path is normally well below one second.
        self.assertLess(elapsed, 5.0)

    def test_repeated_small_morphology_is_pixel_exact(self):
        source = Image.effect_noise((120, 80), 40).convert("RGB")
        for text_is_dark, filter_type in (
            (True, ImageFilter.MaxFilter),
            (False, ImageFilter.MinFilter),
        ):
            expected = source.filter(filter_type(15)).filter(
                ImageFilter.GaussianBlur(1.8)
            )
            actual = render.erase_text(source, text_is_dark, 42)
            self.assertIsNotNone(actual)
            self.assertIsNone(ImageChops.difference(expected, actual).getbbox())

    def test_c_backed_background_run_matches_byte_fallback(self):
        image = Image.new("RGB", (80, 60), (235, 237, 241))
        ImageDraw.Draw(image).rectangle((0, 45, 79, 59), fill=(30, 32, 36))
        raw = image.tobytes()
        rect = QRect(10, 10, 40, 15)
        background = QColor(235, 237, 241)
        fallback = render._RGBPixels(image.width, image.height, raw)
        fast = render._RGBPixels(image.width, image.height, raw, image)
        self.assertEqual(
            render._bg_run(fast, rect, background, 59, True),
            render._bg_run(fallback, rect, background, 59, True),
        )


class LifetimeRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.qapp = application()

    def test_closed_block_editors_are_deleted_instead_of_accumulating(self):
        qapp, window, _home = result_fixture()
        controller = App.__new__(App)
        controller.cfg = MemoryConfig({"appearance": {"auto_copy": False}})
        controller.qapp = qapp
        controller._request_serial = 0
        controller._workers = []
        controller._pending = None
        controller.worker = None
        controller._editor = None
        controller.notify = Mock()
        blocks = [Block([Line("Hello", 0, 0, 80, 20)])]
        image = Image.new("RGB", (100, 30), "white")
        controller._last = {
            "image": image,
            "window": window,
            "blocks": blocks,
            "translations": ["你好"],
            "targets": ["zh-Hans"],
            "target_overrides": [None],
        }
        references = []
        try:
            for _ in range(5):
                controller._open_block_editor(window)
                dialog = controller._editor
                self.assertIsInstance(dialog, BlockEditorDialog)
                references.append(weakref.ref(dialog))
                dialog.reject()
                del dialog
                QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
                qapp.processEvents()
                self.assertEqual(window.findChildren(BlockEditorDialog), [])
            gc.collect()
            self.assertTrue(all(reference() is None for reference in references))
        finally:
            window.close()
            QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)

    def test_closed_result_window_wrapper_is_collectable(self):
        qapp = application()
        controller = App.__new__(App)
        controller.qapp = qapp
        controller.cfg = MemoryConfig({"appearance": {"auto_copy": False}})
        controller.result = None
        controller.minimized_result = None
        controller.worker = None
        controller._editor = None
        controller._pending = None
        controller._queued_worker = None
        controller._last = {}
        controller._capture_serial = 0
        controller._start_worker = lambda *_args, **_kwargs: None

        screen = qapp.primaryScreen()
        controller._on_selected(
            screen,
            (0, 0, 64, 40),
            Image.new("RGB", (64, 40), "white"),
        )
        window = controller.result
        reference = weakref.ref(window)
        window.close()
        del window
        QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
        qapp.processEvents()
        gc.collect()
        self.assertIsNone(reference())

    def test_worker_releases_pixels_before_waiting_for_translation(self):
        image = Image.new("RGB", (3840, 2160), "white")
        cfg = MemoryConfig(
            {"translator": {"provider": "fake", "fake": {}}, "lang": {"zh_target": "en"}}
        )
        worker = TranslateWorker(image, cfg)
        engine = Mock()

        def translate(_texts, _target, _source):
            self.assertIsNone(worker._image)
            return ["你好"]

        engine.translate.side_effect = translate
        with (
            patch("screentrans.worker.ocr.recognize", return_value=[Line("Hello", 0, 0, 80, 20)]),
            patch("screentrans.worker.translators.build", return_value=engine),
        ):
            worker.run()

        self.assertIsNone(worker._image)
        engine.close.assert_called_once_with()

    def test_retry_worker_never_keeps_an_unused_image(self):
        image = Image.new("RGB", (3840, 2160), "white")
        block = Block([Line("Hello", 0, 0, 80, 20)])
        worker = TranslateWorker(image, MemoryConfig(), [block])
        self.assertIsNone(worker._image)

    def test_cancelled_worker_does_not_enter_ocr(self):
        worker = TranslateWorker(Image.new("RGB", (3840, 2160), "white"), MemoryConfig())
        worker.cancel()
        with patch("screentrans.worker.ocr.recognize") as recognize:
            worker.run()
        recognize.assert_not_called()
        self.assertIsNone(worker._image)

    def test_successful_render_releases_cached_pil_and_restores_on_demand(self):
        qapp, window, _home = result_fixture()
        controller = App.__new__(App)
        controller.qapp = qapp
        controller.cfg = MemoryConfig({"appearance": {"auto_copy": False}})
        image = Image.new("RGB", (100, 30), "white")
        block = Block([Line("Hello", 0, 0, 80, 20)])
        controller._last = {
            "image": image,
            "window": window,
            "blocks": [block],
            "translations": ["你好"],
        }
        canvas = window.original_image()
        try:
            with patch("screentrans.render.render", return_value=(canvas, [])):
                self.assertTrue(controller._render_cached_result(window))
            self.assertIsNone(controller._last["image"])

            with (
                patch("screentrans.render.qimage_to_pil", return_value=image) as restore,
                patch("screentrans.render.render", return_value=(canvas, [])),
            ):
                self.assertTrue(controller._render_cached_result(window))
            restore.assert_called_once()
        finally:
            window.close()
            QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)

    def test_capture_close_releases_cached_device_contexts(self):
        fake = Mock()
        capture._local.sct = fake
        capture.close()
        fake.close.assert_called_once_with()
        self.assertFalse(hasattr(capture._local, "sct"))

    def test_app_shutdown_waits_for_workers_only_within_fixed_bound(self):
        controller = App.__new__(App)
        controller._shutting_down = False
        controller._capture_serial = 0
        controller.overlays = Mock()
        controller._close_editor = Mock()
        controller._pending = None
        controller._queued_worker = None
        controller.worker = None
        worker = Mock()
        worker.isRunning.return_value = True
        controller._workers = [worker]
        controller.hotkey_host = Mock()
        controller.result = None
        controller.minimized_result = None
        controller.settings = None
        controller.updates = None
        controller.tray = Mock()

        with patch("screentrans.capture.close"):
            controller._shutdown()

        worker.cancel.assert_called()
        self.assertEqual(worker.wait.call_count, 1)
        wait_ms = worker.wait.call_args.args[0]
        self.assertGreaterEqual(wait_ms, 0)
        self.assertLessEqual(wait_ms, WORKER_SHUTDOWN_WAIT_MS)


if __name__ == "__main__":
    unittest.main()
