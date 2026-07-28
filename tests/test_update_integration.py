from __future__ import annotations

import os
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from qt_helpers import MemoryConfig
from screentrans import config as config_module
from screentrans.updater import ArtifactInfo, UpdateInfo


with patch.object(
    config_module,
    "CONFIG_PATH",
    Path(__file__).with_name("__missing_config__.json"),
):
    from screentrans import main as main_module


App = main_module.App
REPOSITORY_URL = "https://github.com/reevebyte/ScreenTranslate"


def update_1_0_2() -> UpdateInfo:
    artifact_name = "ScreenTranslate-1.0.2-setup-x64.exe"
    return UpdateInfo(
        version="1.0.2",
        channel="stable",
        release_url=f"{REPOSITORY_URL}/releases/tag/v1.0.2",
        published_at="2026-07-28T12:00:00Z",
        artifact=ArtifactInfo(
            name=artifact_name,
            url=f"{REPOSITORY_URL}/releases/download/v1.0.2/{artifact_name}",
            size=1024,
            sha256="a" * 64,
        ),
    )


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

    def test_update_dialog_connects_install_request_to_app_handler(self):
        self.app.updates = None
        self.app.icon = Mock()
        dialog = Mock()

        with patch("screentrans.ui.update_dialog.UpdateDialog", return_value=dialog):
            created = App._ensure_update_dialog(self.app)

        self.assertIs(created, dialog)
        connected = dialog.installRequested.connect.call_args.args[0]
        self.assertIs(connected.__self__, self.app)
        self.assertIs(connected.__func__, App._on_install_requested)

    def test_install_request_relaunches_helper_then_quits(self):
        self.app.cfg.set("updates.repository_url", REPOSITORY_URL)
        self.app.updates = self.dialog
        self.app.quit = Mock()
        info = update_1_0_2()
        installer = Path("C:/verified/ScreenTranslate-1.0.2-setup-x64.exe")
        arguments = ["--apply-update", "--update-version", "1.0.2"]

        with (
            patch(
                "screentrans.updater.install_helper_arguments",
                return_value=arguments,
            ) as build_arguments,
            patch.object(main_module.os, "getpid", return_value=4321),
            patch.object(main_module.winsys, "relaunch") as relaunch,
        ):
            self.app._on_install_requested(installer, info)

        build_arguments.assert_called_once_with(
            installer,
            info,
            repository_url=REPOSITORY_URL,
            parent_pid=4321,
        )
        relaunch.assert_called_once_with(arguments)
        self.app.quit.assert_called_once_with()
        self.dialog.install_failed.assert_not_called()

    def test_install_request_reports_helper_launch_failure(self):
        self.app.cfg.set("updates.repository_url", REPOSITORY_URL)
        self.app.updates = self.dialog
        self.app.quit = Mock()
        info = update_1_0_2()
        installer = Path("C:/verified/ScreenTranslate-1.0.2-setup-x64.exe")
        arguments = ["--apply-update", "--update-version", "1.0.2"]

        with (
            patch(
                "screentrans.updater.install_helper_arguments",
                return_value=arguments,
            ),
            patch.object(main_module.winsys, "relaunch", side_effect=OSError("blocked")),
            patch.object(main_module, "report_exception"),
        ):
            self.app._on_install_requested(installer, info)

        self.dialog.install_failed.assert_called_once_with("无法启动安装程序（OSError）")
        self.app.quit.assert_not_called()

    def test_apply_update_mode_bypasses_normal_single_instance_startup(self):
        argv = [
            "ScreenTranslate.exe",
            "--apply-update",
            "--update-path",
            r"C:\verified\ScreenTranslate-1.0.2-setup-x64.exe",
            "--update-version",
            "1.0.2",
            "--update-size",
            "1024",
            "--update-sha256",
            "a" * 64,
            "--update-repository",
            REPOSITORY_URL,
            "--parent-pid",
            "4321",
        ]

        with (
            patch.object(main_module.sys, "argv", argv),
            patch("screentrans.updater.run_install_helper") as run_install_helper,
            patch.object(main_module, "_already_running") as already_running,
            patch.object(main_module, "QApplication") as qapplication,
            patch.object(main_module, "App") as app_class,
        ):
            exit_code = main_module.main()

        self.assertEqual(exit_code, 0)
        run_install_helper.assert_called_once_with(argv)
        already_running.assert_not_called()
        qapplication.assert_not_called()
        app_class.assert_not_called()


if __name__ == "__main__":
    unittest.main()
