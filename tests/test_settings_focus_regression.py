from __future__ import annotations

import os
import unittest
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QObject, QPoint, QPointF, Qt, Signal
from PySide6.QtGui import QWheelEvent
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

from qt_helpers import MemoryConfig, application
from screentrans import winsys
from screentrans.ui import settings_window


class _IdleTestThread(QObject):
    done = Signal(bool, str)

    def __init__(self, _provider, _opts):
        super().__init__()
        self.started = False

    def isRunning(self):
        return False

    def start(self):
        self.started = True


class SettingsFocusRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = application()

    def setUp(self):
        cfg = MemoryConfig(
            {
                "translator": {
                    "provider": "microsoft",
                    "microsoft": {
                        "key": "",
                        "region": "eastasia",
                        "endpoint": "https://example.invalid",
                    },
                },
                "lang": {"zh_target": "en"},
                "appearance": {"accent": "#28C76F"},
            }
        )
        with (
            patch.object(settings_window, "available_engines", return_value={}),
            patch.object(settings_window.windows_ocr, "available_languages", return_value=[]),
            patch.object(winsys, "get_autostart", return_value=False),
        ):
            self.window = settings_window.SettingsWindow(cfg)
        self.window.nav.setCurrentRow(1)
        self.window.show()
        self.window.raise_()
        self.window.activateWindow()
        self.app.processEvents()

    def tearDown(self):
        self.window.close()
        self.app.processEvents()

    def test_keyboard_test_action_does_not_focus_or_wheel_change_language(self):
        combo = self.window.zh_target
        original_index = combo.currentIndex()
        self.assertGreater(combo.count(), 1)

        self.window.test_btn.setFocus(Qt.FocusReason.TabFocusReason)
        self.app.processEvents()
        self.assertTrue(self.window.test_btn.hasFocus())

        with patch.object(settings_window, "_TestThread", _IdleTestThread):
            QTest.keyClick(self.window.test_btn, Qt.Key.Key_Space)

        self.assertFalse(self.window.test_btn.isEnabled())
        self.assertFalse(combo.hasFocus())
        self.assertIsNot(QApplication.focusWidget(), combo)

        local = QPoint(10, max(1, combo.height() // 2))
        wheel = QWheelEvent(
            QPointF(local),
            QPointF(combo.mapToGlobal(local)),
            QPoint(0, 0),
            QPoint(0, -120),
            Qt.MouseButton.NoButton,
            Qt.KeyboardModifier.NoModifier,
            Qt.ScrollPhase.ScrollUpdate,
            False,
        )
        QApplication.sendEvent(combo, wheel)
        self.assertEqual(combo.currentIndex(), original_index)

        self.window._on_test_done(True, "OK")

    def test_late_connection_result_is_discarded_after_provider_switch(self):
        original = self.window._current_provider()
        self.window._test_provider = original
        self.window.test_btn.setEnabled(False)
        next_index = (self.window.provider_combo.currentIndex() + 1) % self.window.provider_combo.count()
        self.window.provider_combo.setCurrentIndex(next_index)

        self.window._on_test_done(True, "OLD PROVIDER RESULT")

        self.assertTrue(self.window.test_btn.isEnabled())
        self.assertEqual(self.window.test_status.text(), "")

    def test_shutdown_cancels_and_waits_only_for_fixed_bound(self):
        thread = Mock()
        thread.isRunning.return_value = True
        self.window._test_thread = thread
        self.window._models_thread = None

        self.window.shutdown()

        thread.cancel.assert_called_once_with()
        wait_ms = thread.wait.call_args.args[0]
        self.assertGreaterEqual(wait_ms, 0)
        self.assertLessEqual(wait_ms, settings_window.SHUTDOWN_WAIT_MS)

    def test_save_failure_is_visible_from_the_ocr_page(self):
        self.window.nav.setCurrentRow(2)
        self.window.cfg.save = Mock(side_effect=OSError("locked"))

        self.assertFalse(self.window._save())
        self.app.processEvents()

        self.assertTrue(self.window.save_status.isVisible())
        self.assertIn("配置保存失败", self.window.save_status.text())
        self.assertEqual(self.window.stack.currentIndex(), 2)


if __name__ == "__main__":
    unittest.main()
