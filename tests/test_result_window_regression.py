from __future__ import annotations

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QPoint, QRect, Qt
from PySide6.QtGui import QColor, QImage
from PySide6.QtTest import QSignalSpy, QTest

from qt_helpers import result_fixture, solid_image
from screentrans.result import _SelectableText


class ResultWindowRegressionTests(unittest.TestCase):
    def setUp(self):
        self.app, self.window, self.home = result_fixture()
        self.window.show()
        self.app.processEvents()

    def tearDown(self):
        self.window._spin.stop()
        self.window.close()
        self.app.processEvents()

    def test_expanding_region_does_not_stretch_original_pixels(self):
        original_size = self.window._original.deviceIndependentSize().toSize()
        self.window._busy = False
        self.window._spin.stop()
        self.window.setGeometry(
            QRect(
                self.home.left(),
                self.home.top(),
                self.home.width() + 40,
                self.home.height() + 30,
            )
        )
        self.app.processEvents()

        rendered = QImage(self.window.size(), QImage.Format.Format_ARGB32)
        rendered.fill(Qt.GlobalColor.transparent)
        self.window.render(rendered)

        inside = rendered.pixelColor(self.home.width() // 2, self.home.height() // 2)
        newly_exposed = rendered.pixelColor(
            self.home.width() + 20, self.home.height() // 2
        )
        newly_exposed_below = rendered.pixelColor(
            self.home.width() // 2, self.home.height() + 15
        )
        self.assertGreater(inside.red(), 180)
        self.assertLess(inside.green(), 80)
        self.assertEqual(newly_exposed.alpha(), 0)
        self.assertEqual(newly_exposed_below.alpha(), 0)
        self.assertEqual(
            self.window._original.deviceIndependentSize().toSize(), original_size
        )

    def test_expanding_left_and_top_keeps_original_at_its_screen_position(self):
        self.window._busy = False
        self.window._spin.stop()
        left_extension = 30
        top_extension = 20
        self.window.setGeometry(
            self.home.adjusted(-left_extension, -top_extension, 0, 0)
        )
        self.app.processEvents()

        rendered = QImage(self.window.size(), QImage.Format.Format_ARGB32)
        rendered.fill(Qt.GlobalColor.transparent)
        self.window.render(rendered)

        exposed_left = rendered.pixelColor(
            left_extension // 2,
            top_extension + self.home.height() // 2,
        )
        exposed_top = rendered.pixelColor(
            left_extension + self.home.width() // 2,
            top_extension // 2,
        )
        original = rendered.pixelColor(
            left_extension + self.home.width() // 2,
            top_extension + self.home.height() // 2,
        )
        self.assertEqual(exposed_left.alpha(), 0)
        self.assertEqual(exposed_top.alpha(), 0)
        self.assertEqual(original.alpha(), 255)
        self.assertGreater(original.red(), 180)
        self.assertLess(original.green(), 80)

    def test_end_recapture_makes_the_expanded_region_opaque(self):
        expanded = QRect(
            self.home.left(),
            self.home.top(),
            self.home.width() + 40,
            self.home.height() + 30,
        )
        self.window._busy = False
        self.window._spin.stop()
        self.window.setGeometry(expanded)
        self.app.processEvents()

        before = QImage(self.window.size(), QImage.Format.Format_ARGB32)
        before.fill(Qt.GlobalColor.transparent)
        self.window.render(before)
        sample = QPoint(self.home.width() + 20, self.home.height() // 2)
        self.assertEqual(before.pixelColor(sample).alpha(), 0)

        dpr = self.window.screen().devicePixelRatio() or 1.0
        recaptured = solid_image(
            expanded.width(),
            expanded.height(),
            QColor(20, 80, 210),
            dpr,
        )
        self.window.end_recapture(recaptured, expanded, self.window.screen())
        self.window._busy = False
        self.window._spin.stop()
        self.app.processEvents()

        after = QImage(self.window.size(), QImage.Format.Format_ARGB32)
        after.fill(Qt.GlobalColor.transparent)
        self.window.render(after)
        recaptured_pixel = after.pixelColor(sample)
        self.assertEqual(recaptured_pixel.alpha(), 255)
        self.assertGreater(recaptured_pixel.blue(), 180)
        self.assertEqual(self.window._captured, expanded)
        self.assertEqual(self.window._img_pos, expanded.topLeft())

    def test_resize_emits_region_changed_only_when_finished(self):
        spy = QSignalSpy(self.window.regionChanged)
        start = self.window.mapToGlobal(
            QPoint(self.window.width() - 1, self.window.height() // 2)
        )
        self.assertTrue(self.window._begin_resize_at(start))
        self.window._drag_resize_to(
            start + QPoint(35, 0), Qt.KeyboardModifier.NoModifier
        )
        self.assertEqual(spy.count(), 0)
        self.window._finish_resize()
        self.assertEqual(spy.count(), 1)
        self.assertEqual(self.window.width(), self.home.width() + 35)

    def test_home_recapture_decision_uses_last_captured_region(self):
        spy = QSignalSpy(self.window.regionChanged)

        # Moving only repositions the already captured image and must not recapture.
        self.window._nudge(QPoint(5, 0))
        self.window.reset_geometry()
        self.assertEqual(spy.count(), 0)

        # Even at home geometry, Home must recapture if the latest capture differs.
        self.window._captured = self.home.translated(1, 0)
        self.assertEqual(self.window.geometry(), self.home)
        self.window.reset_geometry()
        self.assertEqual(spy.count(), 1)

    def test_text_label_at_window_edge_can_start_resize(self):
        label = _SelectableText("selectable text", self.window)
        label.setGeometry(self.window.rect())
        label.show()
        label.raise_()
        self.app.processEvents()

        QTest.mousePress(
            label,
            Qt.MouseButton.LeftButton,
            Qt.KeyboardModifier.NoModifier,
            QPoint(1, label.height() // 2),
        )
        self.assertIn("l", self.window._resize_zone)
        self.assertTrue(label._edge_resize)
        QTest.mouseRelease(
            label,
            Qt.MouseButton.LeftButton,
            Qt.KeyboardModifier.NoModifier,
            QPoint(1, label.height() // 2),
        )

    def test_resized_rect_is_clipped_to_current_screen(self):
        bounds = self.window.screen().geometry()
        for zone, delta in (
            ("tl", QPoint(-100000, -100000)),
            ("br", QPoint(100000, 100000)),
            ("l", QPoint(-100000, 0)),
            ("b", QPoint(0, 100000)),
        ):
            with self.subTest(zone=zone):
                self.window._resize_geo = QRect(self.home)
                self.window._resize_zone = zone
                resized = self.window._resized_rect(delta, False)
                self.assertFalse(resized.isEmpty())
                self.assertTrue(bounds.contains(resized.topLeft()))
                self.assertTrue(bounds.contains(resized.bottomRight()))

    def test_edit_entry_is_enabled_after_result_and_has_e_shortcut(self):
        spy = QSignalSpy(self.window.editRequested)
        self.assertFalse(self.window._bar._edit.isEnabled())

        self.window.set_result(self.window.original_image(), "translated", [])
        self.assertTrue(self.window._bar._edit.isEnabled())

        QTest.keyClick(self.window, Qt.Key.Key_E)
        self.assertEqual(spy.count(), 1)
        self.assertFalse(self.window._bar._edit.isEnabled())
        self.window._bar._edit.click()
        self.assertEqual(spy.count(), 1)

        self.window.editing_finished()
        self.assertTrue(self.window._bar._edit.isEnabled())
        self.window._bar._edit.click()
        self.assertEqual(spy.count(), 2)

    def test_repeated_edit_requests_are_ignored_until_editing_finishes(self):
        spy = QSignalSpy(self.window.editRequested)
        self.window.set_result(self.window.original_image(), "translated", [])

        self.window.request_edit()
        self.window.request_edit()
        QTest.keyClick(self.window, Qt.Key.Key_E)

        self.assertTrue(self.window._editing)
        self.assertEqual(spy.count(), 1)
        self.assertFalse(self.window._bar._edit.isEnabled())

    def test_edit_shortcut_does_not_open_without_a_completed_result(self):
        spy = QSignalSpy(self.window.editRequested)
        self.window.set_error("translation failed")
        QTest.keyClick(self.window, Qt.Key.Key_E)
        self.assertEqual(spy.count(), 0)

    def test_edit_remains_available_if_a_retry_fails(self):
        self.window.set_result(self.window.original_image(), "translated", [])
        self.window.request_retry()
        self.window.set_error("retry failed")
        self.assertTrue(self.window._bar._edit.isEnabled())

    def test_editing_finished_rearms_timeout_close_mode(self):
        self.window._close_mode = "timeout"
        self.window.set_result(self.window.original_image(), "translated", [])
        self.assertIsNotNone(self.window._close_timer)

        self.window.request_edit()
        self.assertIsNone(self.window._close_timer)
        self.assertFalse(self.window._bar._edit.isEnabled())
        self.window.editing_finished()
        self.assertIsNotNone(self.window._close_timer)
        self.assertTrue(self.window._close_timer.isActive())
        self.assertTrue(self.window._bar._edit.isEnabled())

    def test_single_block_redraw_does_not_close_window_while_editor_is_open(self):
        self.window._close_mode = "timeout"
        image = self.window.original_image()
        self.window.set_result(image, "translated", [])
        self.window.request_edit()

        self.window.set_result(image, "updated block", [])

        self.assertTrue(self.window._editing)
        self.assertIsNone(self.window._close_timer)
        self.assertFalse(self.window._bar._edit.isEnabled())
        self.window.editing_finished()
        self.assertIsNotNone(self.window._close_timer)

    def test_minimize_stops_timeout_until_restored_from_tray(self):
        self.window._close_mode = "timeout"
        self.window.set_result(self.window.original_image(), "translated", [])
        minimized = QSignalSpy(self.window.minimized)
        self.assertIsNotNone(self.window._close_timer)

        self.window.minimize()

        self.assertTrue(self.window._minimized)
        self.assertFalse(self.window.isVisible())
        self.assertIsNone(self.window._close_timer)
        self.assertEqual(minimized.count(), 1)
        self.window.minimize()
        self.assertEqual(minimized.count(), 1)

        self.window.restore_from_tray()

        self.assertFalse(self.window._minimized)
        self.assertTrue(self.window.isVisible())
        self.assertIsNotNone(self.window._close_timer)
        self.assertTrue(self.window._close_timer.isActive())

    def test_result_arriving_while_minimized_waits_to_arm_timer(self):
        self.window._close_mode = "timeout"
        self.window.minimize()

        self.window.set_result(self.window.original_image(), "translated", [])

        self.assertTrue(self.window._minimized)
        self.assertIsNone(self.window._close_timer)
        self.window.restore_from_tray()
        self.assertIsNotNone(self.window._close_timer)
        self.assertTrue(self.window._close_timer.isActive())

    def test_editing_finished_while_minimized_arms_only_after_restore(self):
        self.window._close_mode = "timeout"
        self.window.set_result(self.window.original_image(), "translated", [])
        self.window.request_edit()
        self.window.minimize()

        self.window.editing_finished()

        self.assertFalse(self.window._editing)
        self.assertTrue(self.window._minimized)
        self.assertIsNone(self.window._close_timer)
        self.assertTrue(self.window._bar._edit.isEnabled())

        self.window.restore_from_tray()
        self.assertIsNotNone(self.window._close_timer)
        self.assertTrue(self.window._close_timer.isActive())

    def test_restore_arms_leave_mode_but_not_click_or_error_states(self):
        image = self.window.original_image()
        self.window._close_mode = "leave"
        self.window.set_result(image, "translated", [])
        self.window.minimize()
        self.assertIsNone(self.window._leave_timer)
        self.window.restore_from_tray()
        self.assertIsNotNone(self.window._leave_timer)
        self.assertTrue(self.window._leave_timer.isActive())

        self.window._close_mode = "click"
        self.window.minimize()
        self.window.restore_from_tray()
        self.assertIsNone(self.window._leave_timer)
        self.assertIsNone(self.window._close_timer)

        self.window.set_error("failed")
        self.window._close_mode = "timeout"
        self.window.minimize()
        self.window.restore_from_tray()
        self.assertIsNone(self.window._close_timer)


if __name__ == "__main__":
    unittest.main()
