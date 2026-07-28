from __future__ import annotations

import os
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from qt_helpers import MemoryConfig
from screentrans import config as config_module


with patch.object(
    config_module,
    "CONFIG_PATH",
    Path(__file__).with_name("__missing_config__.json"),
):
    from screentrans.main import App


class UpdateIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.app = App.__new__(App)
        self.app.cfg = MemoryConfig(
            {
                "updates": {
                    "manifest_url": "",
                    "repository_url": "",
                    "channel": "stable",
                }
            }
        )
        self.dialog = Mock()
        self.app._ensure_update_dialog = Mock(return_value=self.dialog)

    def test_startup_check_does_not_touch_network_without_manifest_url(self):
        self.app._check_updates_silently()
        self.app._ensure_update_dialog.assert_not_called()

    def test_configured_startup_check_is_silent(self):
        self.app.cfg.set(
            "updates.manifest_url",
            "https://github.com/example/ScreenTranslate/releases/latest/download/update-manifest.json",
        )
        self.app.cfg.set("updates.repository_url", "https://github.com/example/ScreenTranslate")

        self.app._check_updates_silently()

        self.dialog.check_silently.assert_called_once_with()

    def test_manual_entry_shows_dialog_and_checks_configured_url(self):
        self.app.cfg.set(
            "updates.manifest_url",
            "https://github.com/example/ScreenTranslate/releases/latest/download/update-manifest.json",
        )
        self.app.cfg.set("updates.repository_url", "https://github.com/example/ScreenTranslate")

        self.app.open_updates()

        self.dialog.show_front.assert_called_once_with()
        self.dialog.check.assert_called_once_with()

    def test_available_update_changes_tray_label_and_notifies(self):
        self.app.act_update = Mock()
        self.app.notify = Mock()

        self.app._on_update_available(SimpleNamespace(version="1.2.3"))

        self.app.act_update.setText.assert_called_once_with("发现新版本 1.2.3…")
        self.app.notify.assert_called_once()


if __name__ == "__main__":
    unittest.main()
