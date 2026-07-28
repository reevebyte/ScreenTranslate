from __future__ import annotations

import sys
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

from PIL import Image

from screentrans.layout import Block
from screentrans.ocr.base import Line, OcrError, Word
from screentrans.ocr import rapid_ocr, windows_ocr


class OcrConfidenceTests(unittest.TestCase):
    def test_old_positional_constructors_remain_compatible(self):
        word = Word("legacy", 1, 2, 3, 4)
        line = Line("legacy", 1, 2, 3, 4, [word])

        self.assertIsNone(word.confidence)
        self.assertEqual(line.words, [word])
        self.assertIsNone(line.confidence)
        self.assertIsNone(Block([line]).confidence)

    def test_rapidocr_preserves_engine_score_on_word_and_line(self):
        engine = Mock(return_value=(
            [
                ([[1, 2], [11, 2], [11, 8], [1, 8]], "hello", 0.875),
                ([[2, 10], [8, 10], [8, 16], [2, 16]], "world", None),
                ([[3, 17], [9, 17], [9, 19], [3, 19]], "legacy"),
            ],
            None,
        ))
        image = Image.new("RGB", (20, 20), "white")
        image_array = object()

        with patch.object(rapid_ocr, "_get_engine", return_value=engine), patch.object(
            rapid_ocr, "_image_array", return_value=image_array
        ), patch.object(
            rapid_ocr, "_repair_spaces"
        ):
            lines = rapid_ocr.recognize(image)

        engine.assert_called_once_with(image_array)
        self.assertEqual(len(lines), 3)
        self.assertEqual(lines[0].confidence, 0.875)
        self.assertEqual(lines[0].words[0].confidence, 0.875)
        self.assertIsNone(lines[1].confidence)
        self.assertIsNone(lines[1].words[0].confidence)
        self.assertIsNone(lines[2].confidence)
        self.assertIsNone(lines[2].words[0].confidence)

    def test_non_finite_or_invalid_rapidocr_scores_are_not_fabricated(self):
        for raw in (float("nan"), float("inf"), "not-a-score"):
            with self.subTest(raw=raw):
                self.assertIsNone(rapid_ocr._engine_confidence(raw))
        self.assertEqual(rapid_ocr._engine_confidence(0), 0.0)

    def test_missing_optional_numpy_has_a_clear_runtime_error(self):
        image = Image.new("RGB", (1, 1), "white")
        with patch.dict(sys.modules, {"numpy": None}), self.assertRaisesRegex(
            OcrError,
            "缺少 NumPy",
        ):
            rapid_ocr._image_array(image)

    def test_windows_ocr_keeps_confidence_unknown(self):
        rect = SimpleNamespace(x=1, y=2, width=10, height=6)
        ocr_word = SimpleNamespace(text="system", bounding_rect=rect)
        result = SimpleNamespace(
            text_angle=None,
            lines=[SimpleNamespace(words=[ocr_word])],
        )

        class FakeEngine:
            async def recognize_async(self, _bitmap):
                return result

        image = Image.new("RGB", (20, 20), "white")
        with patch.object(windows_ocr, "_get_engine", return_value=FakeEngine()), patch.object(
            windows_ocr, "_to_software_bitmap", return_value=object()
        ):
            lines = windows_ocr._recognize_once(image, "en-US")

        self.assertEqual(len(lines), 1)
        self.assertIsNone(lines[0].confidence)
        self.assertIsNone(lines[0].words[0].confidence)

    def test_block_aggregates_only_known_engine_confidence(self):
        lines = [
            Line("aa", 0, 0, 10, 5, confidence=0.5),
            Line("bbbb", 0, 6, 10, 5, confidence=1.0),
            Line("unknown", 0, 12, 10, 5),
        ]

        self.assertAlmostEqual(Block(lines).confidence, (0.5 * 2 + 1.0 * 4) / 6)


if __name__ == "__main__":
    unittest.main()
