from __future__ import annotations

import json
import logging
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

from build import build_update_source, copy_license_material
from release.generate_manifest import build_manifest
from screentrans import config as config_module
from screentrans.error_logging import (
    configure_error_logger,
    install_exception_hooks,
    redact,
    report_exception,
)


class ManifestTests(unittest.TestCase):
    def test_manifest_hashes_artifact_and_is_reproducible_with_timestamp(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "ScreenTranslate.zip"
            artifact.write_bytes(b"release bytes")
            manifest = build_manifest(
                artifact,
                "1.2.3",
                "https://github.com/example/ScreenTranslate/releases/download/v1.2.3/ScreenTranslate.zip",
                "stable",
                "2026-07-26T12:00:00Z",
            )

        self.assertEqual(manifest["schema_version"], 2)
        self.assertEqual(manifest["published_at"], "2026-07-26T12:00:00Z")
        self.assertEqual(
            manifest["release_url"],
            "https://github.com/example/ScreenTranslate/releases/tag/v1.2.3",
        )
        self.assertEqual(manifest["artifact"]["size"], 13)
        self.assertEqual(
            manifest["artifact"]["sha256"],
            "ff7a5e6429d2c8511521e4abf41cd54a3e525ef4a1f24f8d1c67ede9d17874dd",
        )
        self.assertNotIn("authenticode", manifest["artifact"])
        json.dumps(manifest)

    def test_manifest_rejects_non_https_download(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "artifact.zip"
            artifact.write_bytes(b"x")
            with self.assertRaises(ValueError):
                build_manifest(
                    artifact,
                    "1.0.0",
                    "http://example.invalid/a.zip",
                    "stable",
                    None,
                )

    def test_manifest_rejects_non_semver_numeric_prerelease(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "artifact.exe"
            artifact.write_bytes(b"x")
            with self.assertRaises(ValueError):
                build_manifest(
                    artifact,
                    "1.0.0-01",
                    "https://github.com/example/project/releases/download/v1.0.0-01/artifact.exe",
                    "stable",
                    None,
                )

    def test_manifest_channel_must_match_version_and_release_tag(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "artifact.exe"
            artifact.write_bytes(b"x")
            with self.assertRaisesRegex(ValueError, "preview requires"):
                build_manifest(
                    artifact,
                    "1.0.0",
                    "https://github.com/example/project/releases/download/v1.0.0/artifact.exe",
                    "preview",
                    None,
                )
            with self.assertRaisesRegex(ValueError, "matching artifact"):
                build_manifest(
                    artifact,
                    "1.0.0-rc.1",
                    "https://github.com/example/project/releases/download/v1.0.0/artifact.exe",
                    "preview",
                    None,
                )

    def test_prerelease_manifest_uses_preview_channel_and_exact_release_page(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "artifact.exe"
            artifact.write_bytes(b"x")
            manifest = build_manifest(
                artifact,
                "1.0.0-rc.2",
                "https://github.com/example/project/releases/download/v1.0.0-rc.2/artifact.exe",
                "preview",
                "2026-07-28T12:00:00Z",
            )
        self.assertEqual(manifest["channel"], "preview")
        self.assertEqual(
            manifest["release_url"],
            "https://github.com/example/project/releases/tag/v1.0.0-rc.2",
        )


class BuildUpdateSourceTests(unittest.TestCase):
    def test_github_actions_repository_is_embedded_without_hardcoded_owner(self):
        source = build_update_source(
            {
                "GITHUB_REPOSITORY": "alice/ScreenTranslate",
                "SCREENTRANS_UPDATE_CHANNEL": "preview",
            }
        )
        self.assertEqual(
            source,
            {
                "manifest_url": (
                    "https://github.com/alice/ScreenTranslate/"
                    "releases/latest/download/update-manifest.json"
                ),
                "repository_url": "https://github.com/alice/ScreenTranslate",
                "channel": "preview",
            },
        )

    def test_local_build_without_repository_has_no_update_source(self):
        self.assertIsNone(build_update_source({}))

    def test_invalid_build_repository_or_channel_is_rejected(self):
        with self.assertRaises(ValueError):
            build_update_source({"GITHUB_REPOSITORY": "not-a-repository"})
        with self.assertRaises(ValueError):
            build_update_source(
                {
                    "GITHUB_REPOSITORY": "alice/ScreenTranslate",
                    "SCREENTRANS_UPDATE_CHANNEL": "nightly",
                }
            )


class LicenseDistributionTests(unittest.TestCase):
    def test_build_copies_project_qt_and_dependency_license_material(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            material_root = copy_license_material(target)

            self.assertTrue((material_root / "LICENSE.txt").is_file())
            self.assertTrue((material_root / "THIRD_PARTY_NOTICES.txt").is_file())
            self.assertTrue(
                (material_root / "THIRD_PARTY_LICENSES" / "Qt" / "LGPL-3.0-only.txt").is_file()
            )
            self.assertTrue(
                (material_root / "THIRD_PARTY_LICENSES" / "Qt" / "GPL-3.0-only.txt").is_file()
            )
            requests_dir = material_root / "THIRD_PARTY_LICENSES" / "requests"
            self.assertTrue(any(requests_dir.glob("LICENSE*")))
            self.assertTrue(any(requests_dir.glob("NOTICE*")))


class ConfigSecurityTests(unittest.TestCase):
    def test_frozen_build_reads_embedded_update_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source_path = root / config_module._BUNDLED_UPDATE_SOURCE_NAME
            source_path.write_text(
                json.dumps(
                    {
                        "manifest_url": (
                            "https://github.com/alice/ScreenTranslate/"
                            "releases/latest/download/update-manifest.json"
                        ),
                        "repository_url": "https://github.com/alice/ScreenTranslate",
                        "channel": "stable",
                    }
                ),
                encoding="utf-8",
            )
            with (
                patch.object(config_module.sys, "frozen", True, create=True),
                patch.object(config_module.sys, "_MEIPASS", str(root), create=True),
            ):
                source = config_module._load_bundled_update_source()

        self.assertEqual(source["repository_url"], "https://github.com/alice/ScreenTranslate")
        self.assertEqual(source["channel"], "stable")

    def test_disk_config_cannot_supply_a_third_party_update_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config_path = root / "config.json"
            config_path.write_text(
                json.dumps(
                    {
                        "updates": {
                            "manifest_url": (
                                "https://github.com/attacker/project/"
                                "releases/latest/download/update-manifest.json"
                            ),
                            "repository_url": "https://github.com/attacker/project",
                            "channel": "preview",
                        }
                    }
                ),
                encoding="utf-8",
            )
            with (
                patch.object(config_module, "CONFIG_DIR", root),
                patch.object(config_module, "CONFIG_PATH", config_path),
            ):
                cfg = config_module.Config()

        self.assertEqual(cfg.get("updates.manifest_url"), "")
        self.assertEqual(cfg.get("updates.repository_url"), "")
        self.assertEqual(cfg.get("updates.channel"), "stable")

    def test_config_refuses_to_persist_plaintext_when_encryption_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with (
                patch.object(config_module, "CONFIG_DIR", root),
                patch.object(config_module, "CONFIG_PATH", root / "config.json"),
                patch.object(config_module.dpapi, "encrypt", return_value="must-not-hit-disk"),
            ):
                cfg = config_module.Config()
                cfg.set("translator.microsoft.key", "must-not-hit-disk")
                with self.assertRaises(config_module.dpapi.EncryptionError):
                    cfg.save()
                self.assertFalse((root / "config.json").exists())

    def test_azure_ocr_key_is_encrypted_before_persisting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config_path = root / "config.json"
            with (
                patch.object(config_module, "CONFIG_DIR", root),
                patch.object(config_module, "CONFIG_PATH", config_path),
                patch.object(
                    config_module.dpapi,
                    "encrypt",
                    side_effect=lambda value: config_module.dpapi.PREFIX + "cipher" if value else value,
                ),
            ):
                cfg = config_module.Config()
                cfg.set("ocr.azure_vision.key", "azure-vision-secret")
                cfg.save()

            raw = config_path.read_text(encoding="utf-8")
            self.assertNotIn("azure-vision-secret", raw)
            self.assertIn(config_module.dpapi.PREFIX + "cipher", raw)


class ErrorLoggingTests(unittest.TestCase):
    def tearDown(self):
        logger = logging.getLogger("tests.private-errors")
        for handler in list(logger.handlers):
            handler.close()
            logger.removeHandler(handler)

    def test_redacts_credentials_and_inline_images(self):
        original = (
            "Authorization: Bearer secret-token api_key=topsecret "
            '"key": "azure-secret" Ocp-Apim-Subscription-Key: subscription-secret '
            "nvapi-1234567890 AIza123456789012345678901234567890 "
            "https://alice:secret@example.invalid/ data:image/png;base64,aGVsbG8= "
            "monkey=banana"
        )
        sanitized = redact(original)
        for secret in (
            "secret-token",
            "topsecret",
            "azure-secret",
            "subscription-secret",
            "nvapi-1234567890",
            "AIza123456789012345678901234567890",
            "alice:secret",
            "aGVsbG8=",
        ):
            self.assertNotIn(secret, sanitized)
        self.assertIn("monkey=banana", sanitized)

    def test_rotates_and_redacts_exception_log(self):
        with tempfile.TemporaryDirectory() as directory:
            logger = configure_error_logger(
                Path(directory),
                max_bytes=300,
                backup_count=2,
                logger_name="tests.private-errors",
            )
            for _ in range(8):
                try:
                    raise RuntimeError("token=do-not-write data:image/png;base64,aGVsbG8=")
                except RuntimeError as exc:
                    report_exception(logger, "translator.request", exc)
            for handler in logger.handlers:
                handler.flush()

            logs = list(Path(directory).glob("errors.log*"))
            combined = "".join(path.read_text(encoding="utf-8") for path in logs)
            for handler in list(logger.handlers):
                handler.close()
                logger.removeHandler(handler)

        self.assertLessEqual(len(logs), 3)
        self.assertNotIn("do-not-write", combined)
        self.assertNotIn("aGVsbG8=", combined)
        self.assertIn("RuntimeError", combined)

    def test_exception_hooks_log_privately_and_preserve_previous_hooks(self):
        old_sys_hook = sys.excepthook
        old_thread_hook = threading.excepthook
        previous_sys = Mock()
        previous_thread = Mock()
        sys.excepthook = previous_sys
        threading.excepthook = previous_thread
        try:
            with tempfile.TemporaryDirectory() as directory:
                logger = configure_error_logger(
                    Path(directory),
                    logger_name="tests.private-errors",
                )
                hooks = install_exception_hooks(logger)
                try:
                    try:
                        raise RuntimeError("api_key=never-persist-this")
                    except RuntimeError as exc:
                        sys.excepthook(type(exc), exc, exc.__traceback__)
                        threading.excepthook(
                            SimpleNamespace(
                                exc_type=type(exc),
                                exc_value=exc,
                                exc_traceback=exc.__traceback__,
                                thread=None,
                            )
                        )
                    for handler in logger.handlers:
                        handler.flush()
                    content = (Path(directory) / "errors.log").read_text(encoding="utf-8")
                finally:
                    hooks.restore()
                    for handler in list(logger.handlers):
                        handler.close()
                        logger.removeHandler(handler)
        finally:
            sys.excepthook = old_sys_hook
            threading.excepthook = old_thread_hook

        self.assertNotIn("never-persist-this", content)
        self.assertIn("process.unhandled", content)
        self.assertIn("thread.unhandled", content)
        previous_sys.assert_called_once()
        previous_thread.assert_called_once()


if __name__ == "__main__":
    unittest.main()
