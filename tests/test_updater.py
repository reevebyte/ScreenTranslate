from __future__ import annotations

import json
import unittest
from threading import Event
from types import SimpleNamespace
from unittest.mock import Mock, patch

from PySide6.QtCore import QThread, QUrl

from qt_helpers import MemoryConfig, application
from screentrans import updater
from screentrans.updater import (
    ArtifactInfo,
    MANIFEST_LIMIT,
    UpdateError,
    UpdateInfo,
    check_for_update,
    github_manifest_repository,
    is_newer_version,
    parse_manifest,
)
from screentrans.ui.update_dialog import SHUTDOWN_WAIT_MS, UpdateDialog, _UpdateThread


REPOSITORY_URL = "https://github.com/example/ScreenTranslate"
MANIFEST_URL = f"{REPOSITORY_URL}/releases/latest/download/update-manifest.json"


class FakeResponse:
    def __init__(self, body: bytes, url: str = MANIFEST_URL, headers=None, history=None):
        self.content = body
        self.url = url
        self.headers = headers or {}
        self.history = history or []
        self.closed = False

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size: int):
        for offset in range(0, len(self.content), max(1, chunk_size // 2)):
            yield self.content[offset:offset + max(1, chunk_size // 2)]

    def close(self):
        self.closed = True


class FakeSession:
    def __init__(self, response: FakeResponse):
        self.response = response
        self.calls = []

    def get(self, url, **kwargs):
        self.calls.append((url, kwargs))
        return self.response


def manifest_for(version: str = "1.1.0", channel: str = "stable") -> dict:
    artifact_name = f"ScreenTranslate-{version}-setup-x64.exe"
    return {
        "schema_version": 2,
        "product": "ScreenTranslate",
        "version": version,
        "channel": channel,
        "published_at": "2026-07-28T12:00:00Z",
        "release_url": f"{REPOSITORY_URL}/releases/tag/v{version}",
        "platform": {"os": "windows", "arch": "x86_64", "minimum": "10"},
        "artifact": {
            "name": artifact_name,
            "url": f"{REPOSITORY_URL}/releases/download/v{version}/{artifact_name}",
            "size": 123,
            "sha256": "0" * 64,
        },
    }


class VersionTests(unittest.TestCase):
    def test_semver_order_handles_prereleases(self):
        self.assertTrue(is_newer_version("1.0.1", "1.0.0"))
        self.assertTrue(is_newer_version("1.0.0", "1.0.0-rc.2"))
        self.assertTrue(is_newer_version("1.0.0-rc.10", "1.0.0-rc.2"))
        self.assertFalse(is_newer_version("1.0.0-beta", "1.0.0"))

    def test_invalid_numeric_prerelease_is_rejected(self):
        with self.assertRaises(UpdateError):
            is_newer_version("1.0.0-01", "1.0.0")


class ManifestTests(unittest.TestCase):
    def test_check_returns_only_newer_release(self):
        response = FakeResponse(json.dumps(manifest_for("1.2.0")).encode())
        info = check_for_update(
            MANIFEST_URL,
            repository_url=REPOSITORY_URL,
            current_version="1.0.0",
            session=FakeSession(response),
        )
        self.assertEqual(
            info,
            UpdateInfo(
                "1.2.0",
                "stable",
                f"{REPOSITORY_URL}/releases/tag/v1.2.0",
                "2026-07-28T12:00:00Z",
                ArtifactInfo(
                    "ScreenTranslate-1.2.0-setup-x64.exe",
                    f"{REPOSITORY_URL}/releases/download/v1.2.0/"
                    "ScreenTranslate-1.2.0-setup-x64.exe",
                    123,
                    "0" * 64,
                ),
            ),
        )
        self.assertTrue(response.closed)

        current_response = FakeResponse(json.dumps(manifest_for("1.0.0")).encode())
        self.assertIsNone(
            check_for_update(
                MANIFEST_URL,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                session=FakeSession(current_response),
            )
        )

    def test_source_must_be_supported_github_release_asset(self):
        for url in (
            "https://example.invalid/update-manifest.json",
            "https://github.com:444/example/ScreenTranslate/releases/latest/download/update-manifest.json",
            f"{REPOSITORY_URL}/raw/main/update-manifest.json",
            f"{REPOSITORY_URL}/releases/latest/download/other.json",
        ):
            with self.subTest(url=url), self.assertRaises(UpdateError):
                github_manifest_repository(url)

    def test_source_is_pinned_to_bundled_repository_before_network(self):
        session = Mock()
        with self.assertRaisesRegex(UpdateError, "不属于内置"):
            check_for_update(
                MANIFEST_URL,
                repository_url="https://github.com/other/project",
                current_version="1.0.0",
                session=session,
            )
        session.get.assert_not_called()

    def test_release_page_must_match_repository_and_version(self):
        payload = manifest_for()
        payload["release_url"] = "https://github.com/attacker/project/releases/tag/v1.1.0"
        with self.assertRaisesRegex(UpdateError, "不属于配置"):
            parse_manifest(payload, REPOSITORY_URL)

        payload = manifest_for()
        payload["release_url"] = f"{REPOSITORY_URL}/releases/tag/v9.9.9"
        with self.assertRaisesRegex(UpdateError, "不属于配置"):
            parse_manifest(payload, REPOSITORY_URL)

    def test_artifact_metadata_is_exposed_and_sha256_is_normalized(self):
        payload = manifest_for()
        payload["artifact"]["sha256"] = "A" * 64
        info = parse_manifest(payload, REPOSITORY_URL)
        self.assertEqual(
            info.artifact,
            ArtifactInfo(
                "ScreenTranslate-1.1.0-setup-x64.exe",
                f"{REPOSITORY_URL}/releases/download/v1.1.0/"
                "ScreenTranslate-1.1.0-setup-x64.exe",
                123,
                "a" * 64,
            ),
        )

    def test_artifact_must_be_an_object_with_a_safe_filename(self):
        for artifact in (
            None,
            [],
            {},
            {
                "name": "../setup.exe",
                "url": f"{REPOSITORY_URL}/releases/download/v1.1.0/setup.exe",
                "size": 123,
                "sha256": "0" * 64,
            },
        ):
            payload = manifest_for()
            payload["artifact"] = artifact
            with self.subTest(artifact=artifact), self.assertRaises(UpdateError):
                parse_manifest(payload, REPOSITORY_URL)

    def test_artifact_url_is_pinned_to_repository_tag_and_filename(self):
        artifact_name = manifest_for()["artifact"]["name"]
        for url in (
            f"http://github.com/example/ScreenTranslate/releases/download/v1.1.0/{artifact_name}",
            f"https://github.com/attacker/ScreenTranslate/releases/download/v1.1.0/{artifact_name}",
            f"{REPOSITORY_URL}/releases/download/v9.9.9/{artifact_name}",
            f"{REPOSITORY_URL}/releases/download/v1.1.0/other.exe",
            f"{REPOSITORY_URL}/releases/download/v1.1.0/{artifact_name}?raw=1",
        ):
            payload = manifest_for()
            payload["artifact"]["url"] = url
            with self.subTest(url=url), self.assertRaises(UpdateError):
                parse_manifest(payload, REPOSITORY_URL)

    def test_artifact_size_must_be_a_nonnegative_integer(self):
        for size in (True, -1, 1.5, "123", 1 << 63):
            payload = manifest_for()
            payload["artifact"]["size"] = size
            with self.subTest(size=size), self.assertRaisesRegex(UpdateError, "大小无效"):
                parse_manifest(payload, REPOSITORY_URL)

    def test_artifact_sha256_must_be_exactly_64_hex_characters(self):
        for digest in (None, "", "0" * 63, "0" * 65, "g" * 64):
            payload = manifest_for()
            payload["artifact"]["sha256"] = digest
            with self.subTest(digest=digest), self.assertRaisesRegex(UpdateError, "SHA-256"):
                parse_manifest(payload, REPOSITORY_URL)

    def test_published_at_must_be_a_timezone_aware_timestamp(self):
        invalid_values = (
            None,
            "not-a-date",
            "2026-07-28T12:00:00",
            "<b>today</b>",
        )
        for published_at in invalid_values:
            payload = manifest_for()
            payload["published_at"] = published_at
            with self.subTest(published_at=published_at), self.assertRaisesRegex(
                UpdateError,
                "发布时间",
            ):
                parse_manifest(payload, REPOSITORY_URL)

    def test_stable_ignores_prerelease_but_preview_accepts_it(self):
        payload = manifest_for("1.1.0-rc.1", "preview")
        stable_response = FakeResponse(json.dumps(payload).encode())
        self.assertIsNone(
            check_for_update(
                MANIFEST_URL,
                repository_url=REPOSITORY_URL,
                channel="stable",
                current_version="1.0.0",
                session=FakeSession(stable_response),
            )
        )

        preview_response = FakeResponse(json.dumps(payload).encode())
        info = check_for_update(
            MANIFEST_URL,
            repository_url=REPOSITORY_URL,
            channel="preview",
            current_version="1.0.0",
            session=FakeSession(preview_response),
        )
        self.assertEqual(info.version, "1.1.0-rc.1")

    def test_manifest_channel_must_match_semver(self):
        with self.assertRaisesRegex(UpdateError, "版本号与发布通道"):
            parse_manifest(manifest_for("1.1.0-rc.1", "stable"), REPOSITORY_URL)
        with self.assertRaisesRegex(UpdateError, "版本号与发布通道"):
            parse_manifest(manifest_for("1.1.0", "preview"), REPOSITORY_URL)

    def test_manifest_is_streamed_and_limited_to_256_kib(self):
        response = FakeResponse(b"{" + b" " * MANIFEST_LIMIT)
        with self.assertRaisesRegex(UpdateError, "清单过大"):
            check_for_update(
                MANIFEST_URL,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                session=FakeSession(response),
            )
        self.assertTrue(response.closed)

    def test_redirects_must_remain_on_github_infrastructure(self):
        response = FakeResponse(
            json.dumps(manifest_for()).encode(),
            url="https://downloads.example.invalid/update-manifest.json",
        )
        with self.assertRaisesRegex(UpdateError, "非 GitHub"):
            check_for_update(
                MANIFEST_URL,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                session=FakeSession(response),
            )

    def test_updater_has_no_artifact_download_or_execution_api(self):
        self.assertFalse(hasattr(updater, "download_update"))
        self.assertFalse(hasattr(updater, "launch_installer"))
        self.assertFalse(hasattr(updater, "verify_authenticode"))


class UpdateDialogTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = application()

    def config(self) -> MemoryConfig:
        return MemoryConfig(
            {
                "appearance": {"accent": "#28C76F"},
                "updates": {
                    "manifest_url": MANIFEST_URL,
                    "repository_url": REPOSITORY_URL,
                    "channel": "stable",
                },
            }
        )

    def test_available_update_only_offers_github_release_page(self):
        dialog = UpdateDialog(self.config())
        info = parse_manifest(manifest_for(), REPOSITORY_URL)
        dialog._checked(info)
        self.assertTrue(dialog.release_btn.isVisibleTo(dialog))
        self.assertTrue(dialog.details.isVisibleTo(dialog))
        self.assertEqual(
            dialog.artifact_value.text(),
            "ScreenTranslate-1.1.0-setup-x64.exe（123 B）",
        )
        self.assertEqual(dialog.sha256_edit.text(), "0" * 64)
        self.assertEqual(dialog.release_btn.text(), "打开 v1.1.0 发布页面")
        self.assertFalse(hasattr(dialog, "download_btn"))
        self.assertFalse(hasattr(dialog, "install_btn"))
        self.assertFalse(hasattr(dialog, "progress"))
        self.assertFalse(hasattr(dialog, "url_edit"))
        dialog.close()

    def test_sha256_copy_uses_only_validated_metadata(self):
        dialog = UpdateDialog(self.config())
        dialog._checked(parse_manifest(manifest_for(), REPOSITORY_URL))
        clipboard = Mock()
        with patch(
            "screentrans.ui.update_dialog.QApplication.clipboard",
            return_value=clipboard,
        ):
            dialog._copy_sha256()
        clipboard.setText.assert_called_once_with("0" * 64)
        self.assertEqual(dialog.copy_sha_btn.text(), "已复制")
        dialog.close()

    def test_open_release_uses_system_browser(self):
        dialog = UpdateDialog(self.config())
        info = parse_manifest(manifest_for(), REPOSITORY_URL)
        dialog._checked(info)
        opened = []
        with patch(
            "screentrans.ui.update_dialog.QDesktopServices.openUrl",
            side_effect=lambda url: opened.append(url) or True,
        ):
            dialog.open_release()
        self.assertEqual(opened, [QUrl(info.release_url)])
        dialog.close()

    def test_shutdown_cancels_and_waits_only_for_the_fixed_bound(self):
        worker = Mock()
        worker.isRunning.return_value = True
        worker.wait.return_value = False
        holder = SimpleNamespace(_thread=worker, _cancelled_thread=None)

        self.assertFalse(UpdateDialog.shutdown(holder))
        worker.cancel.assert_called_once_with()
        worker.wait.assert_called_once_with(SHUTDOWN_WAIT_MS)

        self.assertFalse(UpdateDialog.shutdown(holder))
        worker.cancel.assert_called_once_with()
        worker.wait.assert_called_once_with(SHUTDOWN_WAIT_MS)


class UpdateThreadTests(unittest.TestCase):
    def test_native_worker_is_daemon_and_wait_can_time_out(self):
        started = Event()
        release = Event()
        worker = _UpdateThread(
            "check",
            url=MANIFEST_URL,
            repository_url=REPOSITORY_URL,
        )

        def block_until_released():
            started.set()
            release.wait()

        worker.run = block_until_released
        try:
            worker.start()
            self.assertTrue(started.wait(1))
            self.assertIsNotNone(worker._native_thread)
            self.assertTrue(worker._native_thread.daemon)
            self.assertNotIsInstance(worker, QThread)
            self.assertFalse(worker.wait(1))
            self.assertTrue(worker.isRunning())
        finally:
            release.set()
            self.assertTrue(worker.wait(1000))

    def test_cancellation_suppresses_business_signals(self):
        worker = _UpdateThread(
            "check",
            url=MANIFEST_URL,
            repository_url=REPOSITORY_URL,
        )
        checked = []
        failed = []
        worker.checked.connect(checked.append)
        worker.failed.connect(failed.append)
        worker.cancel()
        with patch("screentrans.ui.update_dialog.check_for_update", return_value=object()):
            worker.run()
        self.assertEqual(checked, [])

        with patch(
            "screentrans.ui.update_dialog.check_for_update",
            side_effect=RuntimeError("cancelled failure"),
        ):
            worker.run()
        self.assertEqual(failed, [])

    def test_finished_signal_carries_worker_even_when_run_raises(self):
        worker = _UpdateThread(
            "check",
            url=MANIFEST_URL,
            repository_url=REPOSITORY_URL,
        )
        finished = []
        worker.finished.connect(finished.append)

        def fail():
            raise RuntimeError("boom")

        worker.run = fail
        with self.assertRaisesRegex(RuntimeError, "boom"):
            worker._run_wrapper()
        self.assertEqual(finished, [worker])


if __name__ == "__main__":
    unittest.main()
