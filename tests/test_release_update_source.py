import json
import tempfile
import unittest
from pathlib import Path

from build import build_update_source
from release.check_update_source import (
    UpdateSourceError,
    expected_update_source,
    main,
    validate_update_source,
)


class PackagedUpdateSourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.source = Path(self.temporary.name) / "screentrans-update-source.json"

    def write_document(self, document):
        self.source.write_text(json.dumps(document), encoding="utf-8")

    def test_accepts_exact_release_source(self):
        expected = expected_update_source("owner/repo", "stable")
        self.write_document(expected)

        actual = validate_update_source(self.source, "owner/repo", "stable")

        self.assertEqual(actual, expected)

    def test_expected_source_matches_build_output_for_each_channel(self):
        for channel in ("stable", "preview"):
            with self.subTest(channel=channel):
                built = build_update_source(
                    {
                        "GITHUB_REPOSITORY": "owner/repo",
                        "SCREENTRANS_UPDATE_CHANNEL": channel,
                    }
                )
                self.assertEqual(
                    expected_update_source("owner/repo", channel),
                    built,
                )

    def test_rejects_wrong_manifest_url(self):
        document = expected_update_source("owner/repo", "stable")
        document["manifest_url"] = (
            "https://github.com/other/repo/releases/latest/download/"
            "update-manifest.json"
        )
        self.write_document(document)

        with self.assertRaises(UpdateSourceError):
            validate_update_source(self.source, "owner/repo", "stable")

    def test_rejects_wrong_channel_and_extra_fields(self):
        document = expected_update_source("owner/repo", "preview")
        document["unexpected"] = "value"
        self.write_document(document)

        with self.assertRaises(UpdateSourceError):
            validate_update_source(self.source, "owner/repo", "stable")

    def test_rejects_duplicate_json_fields(self):
        self.source.write_text(
            '{"manifest_url":"first","manifest_url":"second"}',
            encoding="utf-8",
        )

        with self.assertRaisesRegex(UpdateSourceError, "duplicate JSON field"):
            validate_update_source(self.source, "owner/repo", "stable")

    def test_cli_fails_when_packaged_source_is_missing(self):
        exit_code = main(
            [
                "--source",
                str(self.source),
                "--repository",
                "owner/repo",
                "--channel",
                "stable",
            ]
        )

        self.assertEqual(exit_code, 1)


if __name__ == "__main__":
    unittest.main()
