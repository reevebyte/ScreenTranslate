from __future__ import annotations

import unittest
from unittest.mock import patch

from screentrans import winsys


class AutostartTests(unittest.TestCase):
    def test_registry_entry_must_match_current_existing_launch_files(self):
        argv = [r"C:\Apps\ScreenTranslate\ScreenTranslate.exe"]
        command = '"C:\\Apps\\ScreenTranslate\\ScreenTranslate.exe"'
        with (
            patch.object(winsys, "launch_argv", return_value=argv),
            patch.object(winsys, "_registered_autostart_command", return_value=command),
            patch("os.path.isfile", return_value=True),
        ):
            self.assertTrue(winsys.get_autostart())

    def test_stale_or_missing_launch_target_is_not_enabled(self):
        argv = [r"C:\New\ScreenTranslate.exe"]
        with (
            patch.object(winsys, "launch_argv", return_value=argv),
            patch.object(
                winsys,
                "_registered_autostart_command",
                return_value='"C:\\Old\\ScreenTranslate.exe"',
            ),
            patch("os.path.isfile", return_value=True),
        ):
            self.assertFalse(winsys.get_autostart())

        with (
            patch.object(winsys, "launch_argv", return_value=argv),
            patch.object(
                winsys,
                "_registered_autostart_command",
                return_value='"C:\\New\\ScreenTranslate.exe"',
            ),
            patch("os.path.isfile", return_value=False),
        ):
            self.assertFalse(winsys.get_autostart())

    def test_enabled_config_repairs_stale_registry_entry(self):
        with (
            patch.object(winsys, "get_autostart", return_value=False),
            patch.object(winsys, "set_autostart") as write,
        ):
            self.assertTrue(winsys.ensure_autostart(True))
            write.assert_called_once_with(True)

        with patch.object(winsys, "set_autostart") as write:
            self.assertFalse(winsys.ensure_autostart(False))
            write.assert_not_called()


if __name__ == "__main__":
    unittest.main()
