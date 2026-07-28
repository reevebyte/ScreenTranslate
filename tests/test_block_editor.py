from __future__ import annotations

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import Qt
from PySide6.QtTest import QTest

from qt_helpers import application
from screentrans.layout import Block
from screentrans.ocr.base import Line
from screentrans.ui.block_editor import BlockEditorDialog


def _block(text: str, y: float = 0, confidence: float | None = None) -> Block:
    return Block([Line(text, 0, y, 120, 20, confidence=confidence)])


class BlockTextOverrideTests(unittest.TestCase):
    def test_human_override_preserves_recognized_text_and_can_be_reset(self):
        block = Block(
            [
                Line("Screen", 0, 0, 70, 20),
                Line("translation", 0, 22, 100, 20),
            ]
        )
        self.assertEqual(block.recognized_text, "Screen translation")
        self.assertEqual(block.text, "Screen translation")

        block.edited_text = "Screen translator"
        self.assertEqual(block.text, "Screen translator")
        self.assertEqual(block.recognized_text, "Screen translation")

        block.edited_text = ""
        self.assertEqual(block.text, "")
        block.edited_text = None
        self.assertEqual(block.text, "Screen translation")


class BlockEditorDialogTests(unittest.TestCase):
    def setUp(self):
        self.app = application()
        self.blocks = [
            _block("识别错字", confidence=0.54),
            _block("Second block", 30),
        ]
        self.dialog = BlockEditorDialog(
            self.blocks,
            ["Wrong OCR", "第二块"],
            ["en", "zh-Hans"],
            "en",
        )
        self.dialog.show()
        self.app.processEvents()

    def tearDown(self):
        self.dialog.close()
        self.app.processEvents()

    def test_only_source_or_target_changes_are_returned(self):
        self.assertEqual(self.dialog.changes(), [])
        self.assertFalse(self.dialog.apply_btn.isEnabled())

        self.dialog.source_edit.setPlainText("识别改正")
        self.assertTrue(self.dialog.apply_btn.isEnabled())
        self.dialog.block_list.setCurrentRow(1)
        forced_english = self.dialog.target_combo.findData("en")
        self.dialog.target_combo.setCurrentIndex(forced_english)

        changes = self.dialog.changes()
        self.assertEqual([change.index for change in changes], [0, 1])
        self.assertEqual(changes[0].source_text, "识别改正")
        self.assertTrue(changes[0].source_changed)
        self.assertFalse(changes[0].target_changed)
        self.assertEqual(changes[1].target, "en")
        self.assertFalse(changes[1].source_changed)
        self.assertTrue(changes[1].target_changed)

    def test_apply_and_retranslate_emits_current_block_and_accepts_result(self):
        emitted = []
        self.dialog.blockRetranslateRequested.connect(
            lambda index, source, target: emitted.append((index, source, target))
        )
        self.dialog.source_edit.setPlainText("  校对后的文字  ")
        forced_english = self.dialog.target_combo.findData("en")
        self.dialog.target_combo.setCurrentIndex(forced_english)

        QTest.mouseClick(self.dialog.retranslate_btn, Qt.MouseButton.LeftButton)
        self.app.processEvents()

        self.assertEqual(emitted, [(0, "校对后的文字", "en")])
        self.assertFalse(self.dialog.retranslate_btn.isEnabled())
        self.assertIn("正在重译", self.dialog.status_label.text())
        self.assertEqual(self.dialog.changes(), [])
        self.assertFalse(self.dialog.apply_btn.isEnabled())

        self.dialog.set_translation(0, "Corrected text", "en")
        self.assertEqual(self.dialog.translation_edit.toPlainText(), "Corrected text")
        self.assertTrue(self.dialog.retranslate_btn.isEnabled())
        self.assertIn("强制指定", self.dialog.status_label.text())
        self.assertEqual(self.dialog.changes(), [])
        self.assertFalse(self.dialog.apply_btn.isEnabled())

    def test_error_belongs_to_requested_block_and_can_retry(self):
        self.dialog.source_edit.setPlainText("识别改正")
        QTest.mouseClick(self.dialog.retranslate_btn, Qt.MouseButton.LeftButton)
        self.dialog.set_error(0, "接口暂时不可用")
        self.assertIn("接口暂时不可用", self.dialog.status_label.text())
        self.assertTrue(self.dialog.retranslate_btn.isEnabled())
        self.assertEqual([change.index for change in self.dialog.changes()], [0])
        self.assertTrue(self.dialog.apply_btn.isEnabled())

        self.dialog.block_list.setCurrentRow(1)
        self.assertNotIn("接口暂时不可用", self.dialog.status_label.text())

    def test_apply_remaining_waits_for_every_busy_block(self):
        self.dialog.source_edit.setPlainText("第一块校对")
        QTest.mouseClick(self.dialog.retranslate_btn, Qt.MouseButton.LeftButton)

        self.dialog.block_list.setCurrentRow(1)
        self.dialog.source_edit.setPlainText("Second corrected")
        self.assertEqual([change.index for change in self.dialog.changes()], [1])
        self.assertFalse(self.dialog.apply_btn.isEnabled())

        self.dialog.set_translation(0, "Corrected first block", "en")
        self.assertTrue(self.dialog.apply_btn.isEnabled())

        self.dialog.block_list.setCurrentRow(0)
        self.dialog.source_edit.setPlainText("第一块再次校对")
        QTest.mouseClick(self.dialog.retranslate_btn, Qt.MouseButton.LeftButton)
        self.assertFalse(self.dialog.apply_btn.isEnabled())

        self.dialog.set_error(0, "重译失败")
        self.assertTrue(self.dialog.apply_btn.isEnabled())

    def test_confidence_is_shown_only_when_the_engine_supplies_it(self):
        self.assertIn("54%", self.dialog.confidence_label.text())
        self.assertIn("重点校对", self.dialog.confidence_label.text())

        self.dialog.block_list.setCurrentRow(1)
        self.assertIn("不提供置信度", self.dialog.confidence_label.text())

    def test_all_supported_forced_targets_are_available(self):
        available = {
            self.dialog.target_combo.itemData(index)
            for index in range(self.dialog.target_combo.count())
        }
        self.assertEqual(
            available,
            {
                "auto",
                "zh-Hans",
                "en",
                "ja",
                "ko",
                "fr",
                "de",
                "es",
                "ru",
                "zh-Hant",
            },
        )

    def test_target_modes_are_restored_and_can_change_back_to_auto(self):
        restored = BlockEditorDialog(
            self.blocks,
            ["Wrong OCR", "第二块"],
            ["en", "zh-Hans"],
            "en",
            target_modes=["en", None],
        )
        try:
            self.assertEqual(restored.target_combo.currentData(), "en")
            self.assertEqual(restored.changes(), [])

            restored.target_combo.setCurrentIndex(
                restored.target_combo.findData("auto")
            )
            changes = restored.changes()
            self.assertEqual(len(changes), 1)
            self.assertEqual(changes[0].target, "auto")
            self.assertTrue(changes[0].target_changed)
        finally:
            restored.close()

    def test_rejects_mismatched_parallel_lists(self):
        with self.assertRaises(ValueError):
            BlockEditorDialog(self.blocks, ["only one"], ["en", "zh-Hans"], "en")


if __name__ == "__main__":
    unittest.main()
