from __future__ import annotations

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QPoint, Qt
from PySide6.QtGui import QPixmap

from qt_helpers import application
from screentrans.overlay import (
    CROSSHAIR_ALPHA,
    CROSSHAIR_HALF_LENGTH,
    CROSSHAIR_WIDTH,
    OverlayManager,
    SelectionOverlay,
)


class _FakeOverlay:
    def __init__(self):
        self.released = False
        self.transparent = False
        self.hidden = False
        self.deleted = False

    def releaseKeyboard(self) -> None:
        self.released = True

    def setAttribute(self, attribute, enabled=True) -> None:
        if attribute == Qt.WidgetAttribute.WA_TransparentForMouseEvents:
            self.transparent = enabled

    def hide(self) -> None:
        self.hidden = True

    def deleteLater(self) -> None:
        self.deleted = True


class _RecordingPainter:
    def __init__(self):
        self.lines: list[tuple[int, int, int, int]] = []
        self.pen = None

    def setPen(self, pen) -> None:
        self.pen = pen

    def drawLine(self, x1: int, y1: int, x2: int, y2: int) -> None:
        self.lines.append((x1, y1, x2, y2))


class OverlayCrosshairRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = application()

    def setUp(self):
        screen = self.app.primaryScreen()
        geometry = screen.geometry()
        self.overlay = SelectionOverlay(
            screen,
            QPixmap(1, 1),
            (geometry.left(), geometry.top(), geometry.width(), geometry.height()),
            "#28C76F",
        )

    def tearDown(self):
        self.overlay.close()
        self.app.processEvents()

    def test_crosshair_is_limited_to_short_segments_around_pointer(self):
        center = QPoint(self.overlay.width() // 2, self.overlay.height() // 2)
        self.overlay._current = center
        painter = _RecordingPainter()

        self.overlay._paint_crosshair(painter)

        self.assertEqual(
            painter.lines,
            [
                (
                    center.x() - CROSSHAIR_HALF_LENGTH,
                    center.y(),
                    center.x() + CROSSHAIR_HALF_LENGTH,
                    center.y(),
                ),
                (
                    center.x(),
                    center.y() - CROSSHAIR_HALF_LENGTH,
                    center.x(),
                    center.y() + CROSSHAIR_HALF_LENGTH,
                ),
            ],
        )

    def test_system_cursor_is_hidden_to_avoid_a_second_crosshair(self):
        self.assertEqual(self.overlay.cursor().shape(), Qt.CursorShape.BlankCursor)

    def test_crosshair_is_thick_and_nearly_opaque(self):
        self.overlay._current = QPoint(
            self.overlay.width() // 2,
            self.overlay.height() // 2,
        )
        painter = _RecordingPainter()

        self.overlay._paint_crosshair(painter)

        self.assertEqual(painter.pen.width(), CROSSHAIR_WIDTH)
        self.assertEqual(painter.pen.color().alpha(), CROSSHAIR_ALPHA)

    def test_crosshair_segments_are_clipped_at_overlay_edges(self):
        self.overlay._current = QPoint(3, 4)
        painter = _RecordingPainter()

        self.overlay._paint_crosshair(painter)

        self.assertEqual(
            painter.lines,
            [
                (0, 4, 3 + CROSSHAIR_HALF_LENGTH, 4),
                (3, 0, 3, 4 + CROSSHAIR_HALF_LENGTH),
            ],
        )

    def test_crosshair_is_not_drawn_on_an_overlay_without_the_pointer(self):
        self.overlay._current = QPoint(-20, self.overlay.height() // 2)
        painter = _RecordingPainter()

        self.overlay._paint_crosshair(painter)

        self.assertEqual(painter.lines, [])


class OverlayHandoffOrderTests(unittest.TestCase):
    def setUp(self):
        self.manager = OverlayManager()
        self.overlay = _FakeOverlay()
        self.manager._overlays = [self.overlay]

    def test_result_is_prepared_before_frozen_overlay_closes(self):
        states = []

        def selected(_screen, _rect, _crop):
            states.append((self.manager.active, self.overlay.hidden))

        self.manager._on_selected = selected
        self.manager._handle_selected(None, (0, 0, 10, 10), None)

        self.assertEqual(states, [(True, False)])
        self.assertTrue(self.overlay.released)
        self.assertTrue(self.overlay.transparent)
        self.assertFalse(self.manager.active)
        self.assertTrue(self.overlay.hidden)
        self.assertTrue(self.overlay.deleted)

    def test_result_creation_failure_still_closes_overlay(self):
        def fail(_screen, _rect, _crop):
            raise RuntimeError("boom")

        self.manager._on_selected = fail
        with self.assertRaisesRegex(RuntimeError, "boom"):
            self.manager._handle_selected(None, (0, 0, 10, 10), None)

        self.assertFalse(self.manager.active)
        self.assertTrue(self.overlay.hidden)


if __name__ == "__main__":
    unittest.main()
