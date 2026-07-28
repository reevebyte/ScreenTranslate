from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Event
from types import SimpleNamespace
from unittest.mock import Mock, patch

from PySide6.QtCore import QThread, QUrl
from PySide6.QtWidgets import QMessageBox

from qt_helpers import MemoryConfig, application
from screentrans import __version__, updater
from screentrans.updater import (
    ARTIFACT_LIMIT,
    ArtifactInfo,
    MANIFEST_LIMIT,
    UpdateError,
    UpdateInfo,
    check_for_update,
    download_update,
    github_manifest_repository,
    install_helper_arguments,
    is_newer_version,
    parse_manifest,
    run_install_helper,
    verify_downloaded_installer,
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


def update_for_bytes(content: bytes, version: str = "1.1.0") -> UpdateInfo:
    payload = manifest_for(version)
    payload["artifact"]["size"] = len(content)
    payload["artifact"]["sha256"] = hashlib.sha256(content).hexdigest()
    return parse_manifest(payload, REPOSITORY_URL)


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

    def test_artifact_name_must_match_product_and_version(self):
        payload = manifest_for()
        payload["artifact"]["name"] = "other-setup.exe"
        payload["artifact"]["url"] = (
            f"{REPOSITORY_URL}/releases/download/v1.1.0/other-setup.exe"
        )
        with self.assertRaisesRegex(UpdateError, "文件名与版本不一致"):
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

    def test_artifact_size_must_be_a_bounded_positive_integer(self):
        for size in (True, -1, 0, 1.5, "123", ARTIFACT_LIMIT + 1):
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

    def test_updater_exposes_verified_download_without_authenticode_dependency(self):
        self.assertTrue(hasattr(updater, "download_update"))
        self.assertTrue(hasattr(updater, "verify_downloaded_installer"))
        self.assertFalse(hasattr(updater, "verify_authenticode"))


class DownloadTests(unittest.TestCase):
    def test_download_streams_to_cache_and_reports_progress(self):
        content = b"verified installer bytes"
        info = update_for_bytes(content)
        response = FakeResponse(
            content,
            url=info.artifact.url,
            headers={"Content-Length": str(len(content))},
        )
        session = FakeSession(response)
        progress = []

        with TemporaryDirectory() as folder, patch(
            "screentrans.updater._mark_as_internet_download"
        ) as mark:
            path = download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=Path(folder),
                session=session,
                progress_callback=lambda done, total: progress.append((done, total)),
            )

            self.assertEqual(path.read_bytes(), content)
            self.assertEqual(path.name, info.artifact.name)
            self.assertEqual(progress[-1], (len(content), len(content)))
            mark.assert_called_once()
            marked_path, marked_url = mark.call_args.args
            self.assertEqual(marked_path.parent, path.parent)
            self.assertTrue(marked_path.name.endswith(".part"))
            self.assertEqual(marked_url, info.artifact.url)
            self.assertTrue(response.closed)
            self.assertEqual(session.calls[0][0], info.artifact.url)
            self.assertTrue(session.calls[0][1]["stream"])

    def test_verified_cache_is_reused_without_network(self):
        content = b"cached installer"
        info = update_for_bytes(content)
        with TemporaryDirectory() as folder, patch(
            "screentrans.updater._mark_as_internet_download"
        ):
            first = download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=Path(folder),
                session=FakeSession(FakeResponse(content, url=info.artifact.url)),
            )
            session = Mock()
            second = download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=Path(folder),
                session=session,
            )

        self.assertEqual(second, first)
        session.get.assert_not_called()

    def test_download_prunes_old_versions_and_stale_partial_files(self):
        content = b"new installer"
        info = update_for_bytes(content)
        with TemporaryDirectory() as folder, patch(
            "screentrans.updater._mark_as_internet_download"
        ):
            root = Path(folder)
            old_version = root / "v1.0.0"
            old_version.mkdir()
            (old_version / "old-setup.exe").write_bytes(b"old")
            current_version = root / f"v{info.version}"
            current_version.mkdir()
            stale_partial = current_version / "interrupted.part"
            stale_partial.write_bytes(b"partial")

            path = download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=root,
                session=FakeSession(FakeResponse(content, url=info.artifact.url)),
            )

            self.assertTrue(path.is_file())
            self.assertFalse(old_version.exists())
            self.assertFalse(stale_partial.exists())

    def test_version_cache_reparse_point_is_rejected_before_network(self):
        content = b"installer"
        info = update_for_bytes(content)
        session = Mock()
        with TemporaryDirectory() as folder:
            root = Path(folder)
            version_dir = root / f"v{info.version}"
            version_dir.mkdir()

            with patch(
                "screentrans.updater._is_reparse_point",
                side_effect=lambda path: Path(path).name == version_dir.name,
            ), self.assertRaisesRegex(UpdateError, "重解析点"):
                download_update(
                    info,
                    repository_url=REPOSITORY_URL,
                    current_version="1.0.0",
                    download_dir=root,
                    session=session,
                )

        session.get.assert_not_called()

    def test_size_or_hash_mismatch_leaves_no_installer_or_partial_file(self):
        content = b"wrong bytes"
        cases = []
        size_info = update_for_bytes(content + b"x")
        cases.append((size_info, FakeResponse(content, url=size_info.artifact.url)))
        hash_payload = manifest_for()
        hash_payload["artifact"]["size"] = len(content)
        hash_payload["artifact"]["sha256"] = "0" * 64
        hash_info = parse_manifest(hash_payload, REPOSITORY_URL)
        cases.append((hash_info, FakeResponse(content, url=hash_info.artifact.url)))

        for info, response in cases:
            with self.subTest(message=info.artifact.sha256), TemporaryDirectory() as folder, patch(
                "screentrans.updater._mark_as_internet_download"
            ):
                with self.assertRaises(UpdateError):
                    download_update(
                        info,
                        repository_url=REPOSITORY_URL,
                        current_version="1.0.0",
                        download_dir=Path(folder),
                        session=FakeSession(response),
                    )
                files = [path for path in Path(folder).rglob("*") if path.is_file()]
                self.assertEqual(files, [])

    def test_declared_size_mismatch_is_rejected_before_writing(self):
        content = b"installer"
        info = update_for_bytes(content)
        response = FakeResponse(
            content,
            url=info.artifact.url,
            headers={"Content-Length": str(len(content) + 1)},
        )
        with TemporaryDirectory() as folder, self.assertRaisesRegex(
            UpdateError, "服务器返回的安装包大小"
        ):
            download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=Path(folder),
                session=FakeSession(response),
            )

    def test_download_rejects_redirect_away_from_github(self):
        content = b"installer"
        info = update_for_bytes(content)
        response = FakeResponse(content, url="https://downloads.example.invalid/setup.exe")
        with TemporaryDirectory() as folder, self.assertRaisesRegex(UpdateError, "非 GitHub"):
            download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=Path(folder),
                session=FakeSession(response),
            )

    def test_install_helper_waits_rechecks_and_launches_only_trusted_file(self):
        content = b"installer ready to launch"
        info = update_for_bytes(content)
        waited = []
        launched = []
        with TemporaryDirectory() as folder, patch(
            "screentrans.updater._mark_as_internet_download"
        ):
            root = Path(folder)
            path = download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=root,
                session=FakeSession(FakeResponse(content, url=info.artifact.url)),
            )
            arguments = install_helper_arguments(
                path,
                info,
                repository_url=REPOSITORY_URL,
                parent_pid=1234,
                current_version="1.0.0",
                download_dir=root,
            )
            result = run_install_helper(
                ["ScreenTranslate.exe", *arguments],
                current_version="1.0.0",
                download_dir=root,
                wait_for_exit=waited.append,
                launcher=launched.append,
            )

        self.assertEqual(result, path)
        self.assertEqual(waited, [1234])
        self.assertEqual(launched, [str(path)])

    def test_install_helper_rejects_tampering_and_outside_paths(self):
        content = b"original installer"
        info = update_for_bytes(content)
        with TemporaryDirectory() as folder, patch(
            "screentrans.updater._mark_as_internet_download"
        ):
            root = Path(folder)
            path = download_update(
                info,
                repository_url=REPOSITORY_URL,
                current_version="1.0.0",
                download_dir=root,
                session=FakeSession(FakeResponse(content, url=info.artifact.url)),
            )
            arguments = install_helper_arguments(
                path,
                info,
                repository_url=REPOSITORY_URL,
                parent_pid=1234,
                current_version="1.0.0",
                download_dir=root,
            )
            path.write_bytes(b"tampered")
            launcher = Mock()
            with self.assertRaises(UpdateError):
                run_install_helper(
                    ["ScreenTranslate.exe", *arguments],
                    current_version="1.0.0",
                    download_dir=root,
                    wait_for_exit=lambda _pid: None,
                    launcher=launcher,
                )
            launcher.assert_not_called()

            outside = root.parent / info.artifact.name
            outside.write_bytes(content)
            try:
                with self.assertRaisesRegex(UpdateError, "受信任的更新目录"):
                    verify_downloaded_installer(
                        outside,
                        info,
                        repository_url=REPOSITORY_URL,
                        current_version="1.0.0",
                        download_dir=root,
                    )
            finally:
                outside.unlink()


class ProcessWaitTests(unittest.TestCase):
    @staticmethod
    def kernel32(*, handle=0, wait_result=0):
        return SimpleNamespace(
            OpenProcess=Mock(return_value=handle),
            WaitForSingleObject=Mock(return_value=wait_result),
            CloseHandle=Mock(return_value=True),
        )

    def test_missing_parent_process_is_already_exited(self):
        kernel32 = self.kernel32()
        with (
            patch.object(updater.os, "name", "nt"),
            patch.object(updater.ctypes, "WinDLL", return_value=kernel32),
            patch.object(updater.ctypes, "get_last_error", return_value=87),
        ):
            updater._wait_for_process_exit(1234)

        kernel32.WaitForSingleObject.assert_not_called()
        kernel32.CloseHandle.assert_not_called()

    def test_open_process_failure_is_not_treated_as_process_exit(self):
        kernel32 = self.kernel32()
        with (
            patch.object(updater.os, "name", "nt"),
            patch.object(updater.ctypes, "WinDLL", return_value=kernel32),
            patch.object(updater.ctypes, "get_last_error", return_value=5),
            self.assertRaisesRegex(UpdateError, "Windows 错误 5"),
        ):
            updater._wait_for_process_exit(1234)

        kernel32.WaitForSingleObject.assert_not_called()
        kernel32.CloseHandle.assert_not_called()

    def test_wait_timeout_closes_handle_and_stops_install(self):
        kernel32 = self.kernel32(handle=4321, wait_result=258)
        with (
            patch.object(updater.os, "name", "nt"),
            patch.object(updater.ctypes, "WinDLL", return_value=kernel32),
            self.assertRaisesRegex(UpdateError, "旧版本未能及时退出"),
        ):
            updater._wait_for_process_exit(1234, timeout_seconds=0.01)

        kernel32.WaitForSingleObject.assert_called_once_with(4321, 10)
        kernel32.CloseHandle.assert_called_once_with(4321)


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

    def test_available_update_offers_verified_download_and_release_notes(self):
        dialog = UpdateDialog(self.config())
        info = parse_manifest(manifest_for(), REPOSITORY_URL)
        dialog._checked(info)
        self.assertTrue(dialog.release_btn.isVisibleTo(dialog))
        self.assertTrue(dialog.download_btn.isVisibleTo(dialog))
        self.assertTrue(dialog.details.isVisibleTo(dialog))
        self.assertEqual(
            dialog.artifact_value.text(),
            "ScreenTranslate-1.1.0-setup-x64.exe（123 B）",
        )
        self.assertEqual(dialog.verification_value.text(), "下载完成后自动校验 SHA-256")
        self.assertEqual(dialog.release_btn.text(), "发布说明")
        self.assertEqual(dialog.download_btn.text(), "下载并安装")
        self.assertEqual(dialog.status_title.text(), "发现新版本 1.1.0")
        self.assertFalse(hasattr(dialog, "sha256_edit"))
        self.assertFalse(hasattr(dialog, "copy_sha_btn"))
        self.assertFalse(hasattr(dialog, "url_edit"))
        dialog.close()

    def test_dialog_uses_themed_root_and_clear_initial_state(self):
        dialog = UpdateDialog(self.config())
        self.assertEqual(dialog.objectName(), "Root")
        self.assertEqual(dialog.status_title.text(), "准备检查更新")
        self.assertIn("GitHub Release", dialog.status.text())
        self.assertEqual(dialog.check_btn.text(), "检查更新")
        self.assertFalse(dialog.status_icon.pixmap().isNull())
        dialog.close()

    def test_manual_check_immediately_shows_busy_feedback(self):
        dialog = UpdateDialog(self.config())
        with patch.object(_UpdateThread, "start"):
            dialog.check()

        self.assertEqual(dialog.status_title.text(), "正在检查更新")
        self.assertEqual(dialog.check_btn.text(), "检查中…")
        self.assertFalse(dialog.check_btn.isEnabled())
        dialog.shutdown()
        dialog._thread = None
        dialog.close()

    def test_current_version_has_visible_success_feedback(self):
        dialog = UpdateDialog(self.config())
        worker = _UpdateThread("check")
        dialog._thread = worker

        dialog._checked(None)
        dialog._finished(worker)

        self.assertEqual(dialog.status_title.text(), "已经是最新版")
        self.assertIn(__version__, dialog.status.text())
        self.assertEqual(dialog.check_btn.text(), "重新检查")
        self.assertTrue(dialog.check_btn.isEnabled())
        dialog.close()

    def test_unconfigured_build_explains_why_checking_is_unavailable(self):
        config = self.config()
        config.set("updates.manifest_url", "")
        config.set("updates.repository_url", "")
        dialog = UpdateDialog(config)

        dialog.check()

        self.assertEqual(dialog.status_title.text(), "此构建未配置更新")
        self.assertIn("GitHub Release", dialog.status.text())
        self.assertTrue(dialog.check_btn.isEnabled())
        dialog.close()

    def test_failed_check_has_visible_error_and_retry_action(self):
        dialog = UpdateDialog(self.config())
        worker = _UpdateThread("check")
        dialog._thread = worker

        dialog._failed("无法连接 GitHub。")
        dialog._finished(worker)

        self.assertEqual(dialog.status_title.text(), "检查失败")
        self.assertEqual(dialog.status.text(), "无法连接 GitHub。")
        self.assertEqual(dialog.check_btn.text(), "重试")
        dialog.close()

    def test_download_action_shows_progress_and_can_be_cancelled(self):
        dialog = UpdateDialog(self.config())
        dialog._checked(parse_manifest(manifest_for(), REPOSITORY_URL))

        with patch.object(_UpdateThread, "start"):
            dialog.download_or_install()

        self.assertEqual(dialog._thread.mode, "download")
        self.assertEqual(dialog.status_title.text(), "正在下载 1.1.0")
        self.assertTrue(dialog.progress.isVisibleTo(dialog))
        self.assertEqual(dialog.download_btn.text(), "取消下载")
        self.assertFalse(dialog.check_btn.isEnabled())
        dialog._download_progress(50, 100)
        self.assertEqual(dialog.progress.value(), 500)
        self.assertIn("50%", dialog.progress.format())

        dialog._thread.isRunning = Mock(return_value=True)
        dialog.download_or_install()
        self.assertTrue(dialog._thread.cancel_event.is_set())
        self.assertEqual(dialog.download_btn.text(), "正在取消…")
        dialog._thread = None
        dialog.close()

    def test_verified_download_prompts_then_emits_install_request(self):
        dialog = UpdateDialog(self.config())
        info = parse_manifest(manifest_for(), REPOSITORY_URL)
        dialog._checked(info)
        worker = _UpdateThread("download", info=info)
        dialog._thread = worker
        path = Path("C:/verified/ScreenTranslate-1.1.0-setup-x64.exe")

        with patch("screentrans.ui.update_dialog.QTimer.singleShot") as schedule:
            dialog._downloaded(path)
            dialog._finished(worker)

        schedule.assert_called_once()
        self.assertEqual(dialog.status_title.text(), "安装包已验证")
        self.assertEqual(dialog.verification_value.text(), "大小和 SHA-256 校验已通过")
        self.assertEqual(dialog.download_btn.text(), "安装更新")
        requested = []
        dialog.installRequested.connect(lambda target, update: requested.append((target, update)))
        with patch(
            "screentrans.ui.update_dialog.QMessageBox.question",
            return_value=QMessageBox.StandardButton.Yes,
        ):
            dialog._confirm_install()
        self.assertEqual(requested, [(path, info)])
        self.assertEqual(dialog.status_title.text(), "正在启动安装程序")
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
