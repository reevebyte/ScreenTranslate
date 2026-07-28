from __future__ import annotations

import hashlib
import unittest
from unittest.mock import Mock, patch

from PIL import Image

from qt_helpers import MemoryConfig
from screentrans import ocr
from screentrans.ocr import azure_vision, youdao_cloud
from screentrans.ocr.base import OcrError


class YoudaoCloudOcrTests(unittest.TestCase):
    def test_cloud_engine_is_always_available_and_explicitly_named(self):
        engines = ocr.available_engines()
        self.assertIn("youdao_cloud", engines)
        self.assertIn("截图会上传", engines["youdao_cloud"])

    def test_signature_matches_documented_digest_shape(self):
        image_bytes = b"0123456789abcdefghijABCDEFGHIJ"
        salt = "0.123"
        encoded = __import__("base64").b64encode(image_bytes).decode("ascii")
        expected = hashlib.md5(
            (
                "deskdict"
                + encoded[:10]
                + str(len(encoded))
                + encoded[-10:]
                + salt
                + "VPaHE3kX_vl4BhgYiu2n"
            ).encode()
        ).hexdigest()
        self.assertEqual(youdao_cloud._sign(image_bytes, salt), expected)

    def test_recognize_uploads_without_history_and_parses_regions(self):
        response = Mock()
        response.json.return_value = {
            "errorCode": "0",
            "resRegions": [
                {
                    "boundingBox": "10,20,120,60",
                    "context": "Hello world\nSecond line",
                    "lines": [{"text": "Hello world"}, {"text": "Second line"}],
                },
                {
                    "boundingBox": "12,92,90,22",
                    "context": "",
                    "lines": [{"text": "第二行"}],
                },
            ],
        }
        session = Mock()
        session.post.return_value = response

        lines = youdao_cloud.recognize(
            Image.new("RGB", (200, 100), "white"),
            session=session,
        )

        self.assertEqual(
            [line.text for line in lines],
            ["Hello world", "Second line", "第二行"],
        )
        self.assertEqual(
            (lines[0].x, lines[0].y, lines[0].w, lines[0].h),
            (10, 20, 120, 30),
        )
        self.assertEqual(
            (lines[1].x, lines[1].y, lines[1].w, lines[1].h),
            (10, 50, 120, 30),
        )
        data = session.post.call_args.kwargs["data"]
        self.assertEqual(data["isSaveHistory"], "false")
        self.assertEqual(data["isSyncSaveHistory"], "false")

    def test_dispatcher_uses_cloud_engine(self):
        original = youdao_cloud.recognize
        called = []
        try:
            youdao_cloud.recognize = lambda image: called.append(image.size) or []
            result = ocr.recognize(
                Image.new("RGB", (80, 40), "white"),
                MemoryConfig({"ocr": {"engine": "youdao_cloud"}}),
            )
        finally:
            youdao_cloud.recognize = original
        self.assertEqual(result, [])
        self.assertEqual(called, [(80, 40)])

    def test_invalid_coordinates_are_reported(self):
        with self.assertRaises(OcrError):
            youdao_cloud._parse(
                {
                    "errorCode": "0",
                    "resRegions": [{"boundingBox": "bad", "context": "text"}],
                }
            )


class AzureVisionOcrTests(unittest.TestCase):
    def test_azure_engine_is_available_and_officially_named(self):
        engines = ocr.available_engines()
        self.assertEqual(engines["azure_vision"], "Azure AI Vision OCR（官方云端）")

    def test_read_request_polls_and_parses_line_and_word_polygons(self):
        submitted = Mock()
        submitted.headers = {"Operation-Location": "https://vision.test/result/123"}
        running = Mock()
        running.json.return_value = {"status": "running"}
        succeeded = Mock()
        succeeded.json.return_value = {
            "status": "succeeded",
            "analyzeResult": {
                "readResults": [
                    {
                        "lines": [
                            {
                                "text": "Hello Azure",
                                "boundingBox": [10, 20, 150, 20, 150, 48, 10, 48],
                                "words": [
                                    {
                                        "text": "Hello",
                                        "boundingBox": [10, 20, 70, 20, 70, 48, 10, 48],
                                        "confidence": 0.98,
                                    },
                                    {
                                        "text": "Azure",
                                        "boundingBox": [78, 20, 150, 20, 150, 48, 78, 48],
                                        "confidence": 0.96,
                                    },
                                ],
                            }
                        ]
                    }
                ]
            },
        }
        session = Mock()
        session.post.return_value = submitted
        session.get.side_effect = [running, succeeded]

        with patch.object(azure_vision.time, "sleep"):
            lines = azure_vision.recognize(
                Image.new("RGB", (200, 100), "white"),
                {"endpoint": "https://vision.test/", "key": "secret"},
                session=session,
            )

        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0].text, "Hello Azure")
        self.assertEqual((lines[0].x, lines[0].y, lines[0].w, lines[0].h), (10, 20, 140, 28))
        self.assertEqual([word.text for word in lines[0].words], ["Hello", "Azure"])
        self.assertAlmostEqual(lines[0].confidence, 0.97, places=2)
        self.assertEqual(session.get.call_count, 2)
        self.assertNotIn("secret", session.post.call_args.args[0])

    def test_missing_azure_configuration_fails_before_upload(self):
        with self.assertRaisesRegex(OcrError, "Endpoint"):
            azure_vision.recognize(Image.new("RGB", (80, 40)), {})
        with self.assertRaisesRegex(OcrError, "Key"):
            azure_vision.recognize(
                Image.new("RGB", (80, 40)),
                {"endpoint": "https://vision.test"},
            )

    def test_dispatcher_passes_azure_options(self):
        with patch.object(azure_vision, "recognize", return_value=[]) as recognize:
            result = ocr.recognize(
                Image.new("RGB", (80, 40)),
                MemoryConfig(
                    {
                        "ocr": {
                            "engine": "azure_vision",
                            "azure_vision": {
                                "endpoint": "https://vision.test",
                                "key": "secret",
                            },
                        }
                    }
                ),
            )
        self.assertEqual(result, [])
        recognize.assert_called_once()
        self.assertEqual(recognize.call_args.args[1]["key"], "secret")


if __name__ == "__main__":
    unittest.main()
