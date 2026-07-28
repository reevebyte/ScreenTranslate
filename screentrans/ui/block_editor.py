"""逐块校对 OCR 文本，并把单块重译请求交给主流程。"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QPlainTextEdit,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from .. import langdetect
from ..layout import Block
from .style import BG_CARD, BG_INPUT, LINE, LINE_HI, TEXT_DIM, build_qss


_TARGET_OPTIONS = (
    ("zh-Hans", "简体中文"),
    ("en", "英语"),
    ("ja", "日语"),
    ("ko", "韩语"),
    ("fr", "法语"),
    ("de", "德语"),
    ("es", "西班牙语"),
    ("ru", "俄语"),
    ("zh-Hant", "繁体中文"),
)


@dataclass(frozen=True)
class BlockEdit:
    """一个尚未通过“应用并重译”交给主流程的编辑草稿。"""

    index: int
    source_text: str
    target: str
    translation: str
    source_changed: bool
    target_changed: bool


@dataclass
class _Draft:
    source: str
    translation: str
    actual_target: str
    target_mode: str = "auto"
    applied_source: str = ""
    applied_target_mode: str = "auto"
    submitted_source: str | None = None
    submitted_target_mode: str | None = None
    busy: bool = False
    error: str = ""


class BlockEditorDialog(QDialog):
    """紧凑的 OCR 块校对窗口。

    ``targets`` 是当前译文实际使用的目标语言；``target_modes`` 记录自动或
    强制目标的持久选择，省略时全部按自动处理。网络请求由主流程执行，编辑器
    只发信号并接收结果回填。

    ``Accepted`` 表示调用方应处理 ``changes()`` 中的其余草稿；``Rejected``
    表示丢弃其余草稿。两者都不会撤销已经由 ``blockRetranslateRequested``
    提交并经 ``set_translation`` 回填的单块结果。
    """

    blockRetranslateRequested = Signal(int, str, str)

    def __init__(
        self,
        blocks: Sequence[Block],
        translations: Sequence[str],
        targets: Sequence[str],
        zh_target: str,
        parent: QWidget | None = None,
        target_modes: Sequence[str | None] | None = None,
    ):
        super().__init__(parent)
        if len(blocks) != len(translations) or len(blocks) != len(targets):
            raise ValueError("文本块、译文和目标语言数量必须一致")
        if target_modes is not None and len(target_modes) != len(blocks):
            raise ValueError("文本块和目标语言模式数量必须一致")

        self._blocks = list(blocks)
        self._original_sources = [block.text for block in self._blocks]
        valid_modes = {"auto"} | {code for code, _name in _TARGET_OPTIONS}
        modes = ["auto"] * len(blocks) if target_modes is None else [
            "auto" if mode in (None, "auto") else str(mode)
            for mode in target_modes
        ]
        invalid_modes = [mode for mode in modes if mode not in valid_modes]
        if invalid_modes:
            raise ValueError(f"不支持的目标语言模式：{invalid_modes[0]}")
        self._drafts = [
            _Draft(
                source=source,
                translation=translation,
                actual_target=target,
                target_mode=mode,
                applied_source=source,
                applied_target_mode=mode,
            )
            for source, translation, target, mode in zip(
                self._original_sources, translations, targets, modes
            )
        ]
        self._loading = False
        self._current = -1
        self._zh_target = zh_target

        self.setObjectName("Root")
        self.setWindowTitle("校对识别文字")
        self.setModal(False)
        self.setMinimumSize(590, 410)
        self.resize(680, 480)
        accent = "#4C8DFF"
        if parent is not None and hasattr(parent, "_accent"):
            accent = parent._accent.name()
        self.setStyleSheet(build_qss(accent) + _editor_qss(accent))

        root = QVBoxLayout(self)
        root.setContentsMargins(18, 16, 18, 16)
        root.setSpacing(12)

        title = QLabel("校对识别文字")
        title.setObjectName("PageTitle")
        root.addWidget(title)

        body = QHBoxLayout()
        body.setSpacing(12)
        root.addLayout(body, 1)

        self.block_list = QListWidget()
        self.block_list.setObjectName("BlockList")
        self.block_list.setFixedWidth(190)
        self.block_list.setHorizontalScrollBarPolicy(
            Qt.ScrollBarPolicy.ScrollBarAlwaysOff
        )
        self.block_list.currentRowChanged.connect(self._select_block)
        body.addWidget(self.block_list)

        panel = QFrame()
        panel.setObjectName("EditorPanel")
        detail = QVBoxLayout(panel)
        detail.setContentsMargins(14, 12, 14, 12)
        detail.setSpacing(8)
        body.addWidget(panel, 1)

        self.block_title = QLabel()
        self.block_title.setObjectName("BlockTitle")
        detail.addWidget(self.block_title)

        self.confidence_label = QLabel()
        self.confidence_label.setObjectName("Status")
        detail.addWidget(self.confidence_label)

        source_label = QLabel("识别原文")
        source_label.setObjectName("FieldLabel")
        detail.addWidget(source_label)
        self.source_edit = QPlainTextEdit()
        self.source_edit.setObjectName("SourceEdit")
        self.source_edit.setPlaceholderText("输入正确的原文")
        self.source_edit.setTabChangesFocus(True)
        self.source_edit.textChanged.connect(self._source_changed)
        detail.addWidget(self.source_edit, 1)

        target_row = QHBoxLayout()
        target_row.setSpacing(8)
        target_label = QLabel("目标语言")
        target_label.setObjectName("FieldLabel")
        target_row.addWidget(target_label)
        self.target_combo = QComboBox()
        self.target_combo.addItem(self._auto_label(), "auto")
        for code, name in _TARGET_OPTIONS:
            self.target_combo.addItem(name, code)
        self.target_combo.currentIndexChanged.connect(self._target_changed)
        target_row.addWidget(self.target_combo, 1)
        detail.addLayout(target_row)

        translation_label = QLabel("当前译文")
        translation_label.setObjectName("FieldLabel")
        detail.addWidget(translation_label)
        self.translation_edit = QPlainTextEdit()
        self.translation_edit.setObjectName("TranslationEdit")
        self.translation_edit.setReadOnly(True)
        self.translation_edit.setTabChangesFocus(True)
        detail.addWidget(self.translation_edit, 1)

        action_row = QHBoxLayout()
        action_row.setSpacing(8)
        self.status_label = QLabel()
        self.status_label.setObjectName("Status")
        self.status_label.setWordWrap(True)
        action_row.addWidget(self.status_label, 1)
        self.retranslate_btn = QPushButton("应用并重译")
        self.retranslate_btn.setObjectName("Primary")
        self.retranslate_btn.clicked.connect(self._request_retranslate)
        action_row.addWidget(self.retranslate_btn)
        detail.addLayout(action_row)

        footer = QHBoxLayout()
        footer.addStretch(1)
        close = QPushButton("关闭")
        close.setObjectName("Ghost")
        close.clicked.connect(self.reject)
        footer.addWidget(close)
        self.apply_btn = QPushButton("应用其余更改")
        self.apply_btn.setObjectName("Primary")
        self.apply_btn.clicked.connect(self.accept)
        footer.addWidget(self.apply_btn)
        root.addLayout(footer)

        self._populate_list()
        if self._drafts:
            self.block_list.setCurrentRow(0)
        else:
            panel.setEnabled(False)
        self._refresh_apply_button()

    def _auto_label(self) -> str:
        configured = langdetect.target_display_name(self._zh_target)
        return f"自动（中文→{configured}，其他→简体中文）"

    @staticmethod
    def _preview(text: str, limit: int = 32) -> str:
        one_line = " ".join(text.split()) or "（空白）"
        return one_line if len(one_line) <= limit else one_line[: limit - 1] + "…"

    def _populate_list(self) -> None:
        self.block_list.clear()
        for index, draft in enumerate(self._drafts):
            item = QListWidgetItem(f"{index + 1}  {self._preview(draft.source)}")
            item.setToolTip(draft.source)
            self.block_list.addItem(item)

    def _select_block(self, index: int) -> None:
        if index < 0 or index >= len(self._drafts):
            self._current = -1
            return
        self._current = index
        self._load_current()

    def _load_current(self) -> None:
        if self._current < 0:
            return
        draft = self._drafts[self._current]
        self._loading = True
        self.block_title.setText(
            f"文本块 {self._current + 1} / {len(self._drafts)}"
        )
        self.source_edit.setPlainText(draft.source)
        target_index = self.target_combo.findData(draft.target_mode)
        self.target_combo.setCurrentIndex(max(0, target_index))
        self.translation_edit.setPlainText(draft.translation)
        self._loading = False
        self._refresh_confidence()
        self._refresh_status()
        self._refresh_apply_button()

    def _refresh_confidence(self) -> None:
        confidence = self._blocks[self._current].confidence
        if confidence is None:
            self.confidence_label.setStyleSheet(f"color:{TEXT_DIM};")
            self.confidence_label.setText("当前 OCR 引擎不提供置信度")
        elif confidence < 0.75:
            self.confidence_label.setStyleSheet("color:#E8B44A;")
            self.confidence_label.setText(
                f"识别置信度 {confidence:.0%} · 建议重点校对"
            )
        else:
            self.confidence_label.setStyleSheet(f"color:{TEXT_DIM};")
            self.confidence_label.setText(f"识别置信度 {confidence:.0%}")

    def _source_changed(self) -> None:
        if self._loading or self._current < 0:
            return
        draft = self._drafts[self._current]
        draft.source = self.source_edit.toPlainText()
        draft.error = ""
        item = self.block_list.item(self._current)
        item.setText(f"{self._current + 1}  {self._preview(draft.source)}")
        item.setToolTip(draft.source)
        self._refresh_status()
        self._refresh_apply_button()

    def _target_changed(self) -> None:
        if self._loading or self._current < 0:
            return
        self._drafts[self._current].target_mode = str(
            self.target_combo.currentData()
        )
        self._drafts[self._current].error = ""
        self._refresh_status()
        self._refresh_apply_button()

    def _refresh_status(self) -> None:
        if self._current < 0:
            return
        draft = self._drafts[self._current]
        self.retranslate_btn.setEnabled(not draft.busy)
        self.source_edit.setEnabled(not draft.busy)
        self.target_combo.setEnabled(not draft.busy)
        if draft.busy:
            self.status_label.setStyleSheet(f"color:{TEXT_DIM};")
            self.status_label.setText("正在重译这一块…")
        elif draft.error:
            self.status_label.setStyleSheet("color:#FF6B6B;")
            self.status_label.setText(draft.error)
        elif (
            draft.source != draft.applied_source
            or draft.target_mode != draft.applied_target_mode
        ):
            self.status_label.setStyleSheet("color:#E8B44A;")
            self.status_label.setText("有未应用更改")
        elif draft.target_mode == "auto":
            actual = langdetect.target_display_name(draft.actual_target)
            self.status_label.setStyleSheet(f"color:{TEXT_DIM};")
            self.status_label.setText(f"当前自动译为{actual}")
        else:
            self.status_label.setStyleSheet(f"color:{TEXT_DIM};")
            self.status_label.setText("已强制指定目标语言")

    def _request_retranslate(self) -> None:
        if self._current < 0:
            return
        draft = self._drafts[self._current]
        source = draft.source.strip()
        if not source:
            self.set_error(self._current, "识别原文不能为空")
            return
        draft.source = source
        item = self.block_list.item(self._current)
        item.setText(f"{self._current + 1}  {self._preview(source)}")
        item.setToolTip(source)
        draft.error = ""
        draft.submitted_source = source
        draft.submitted_target_mode = draft.target_mode
        draft.busy = True
        self._refresh_status()
        self._refresh_apply_button()
        self.blockRetranslateRequested.emit(
            self._current, source, draft.target_mode
        )

    def set_translation(self, index: int, text: str, target: str) -> None:
        """主流程完成单块重译后回填，并恢复这一块的操作按钮。"""
        draft = self._draft(index)
        draft.translation = text
        draft.actual_target = target
        if draft.submitted_source is not None:
            draft.applied_source = draft.submitted_source
        if draft.submitted_target_mode is not None:
            draft.applied_target_mode = draft.submitted_target_mode
        draft.submitted_source = None
        draft.submitted_target_mode = None
        draft.error = ""
        draft.busy = False
        if index == self._current:
            self.translation_edit.setPlainText(text)
            self._refresh_status()
        self._refresh_apply_button()

    def set_error(self, index: int, message: str) -> None:
        """主流程单块重译失败时回填；错误只显示在对应块。"""
        draft = self._draft(index)
        draft.error = message
        draft.submitted_source = None
        draft.submitted_target_mode = None
        draft.busy = False
        if index == self._current:
            self._refresh_status()
        self._refresh_apply_button()

    def _refresh_apply_button(self) -> None:
        has_busy_draft = any(draft.busy for draft in self._drafts)
        self.apply_btn.setEnabled(not has_busy_draft and bool(self.changes()))

    def changes(self) -> list[BlockEdit]:
        """返回尚未提交的源文/目标改动，不直接修改 Block。

        已经发出且仍在执行的单块请求不会重复出现；成功回填的请求会更新该块
        的应用基线。失败请求则重新成为待应用改动，用户可以再次提交。
        """
        edits: list[BlockEdit] = []
        for index, draft in enumerate(self._drafts):
            if (
                draft.busy
                and draft.source == draft.submitted_source
                and draft.target_mode == draft.submitted_target_mode
            ):
                continue
            source_changed = draft.source != draft.applied_source
            target_changed = draft.target_mode != draft.applied_target_mode
            if source_changed or target_changed:
                edits.append(
                    BlockEdit(
                        index=index,
                        source_text=draft.source,
                        target=draft.target_mode,
                        translation=draft.translation,
                        source_changed=source_changed,
                        target_changed=target_changed,
                    )
                )
        return edits

    def _draft(self, index: int) -> _Draft:
        if index < 0 or index >= len(self._drafts):
            raise IndexError(f"文本块下标越界：{index}")
        return self._drafts[index]


def _editor_qss(accent: str) -> str:
    return f"""
QListWidget#BlockList {{
    background: {BG_CARD};
    border: 1px solid {LINE};
    border-radius: 6px;
    outline: none;
    padding: 5px;
}}
QListWidget#BlockList::item {{
    min-height: 34px;
    padding: 0 8px;
    border-radius: 5px;
}}
QListWidget#BlockList::item:hover {{ background: #23262D; }}
QListWidget#BlockList::item:selected {{
    background: #2A3039;
    color: #FFFFFF;
}}
QFrame#EditorPanel {{
    background: {BG_CARD};
    border: 1px solid {LINE};
    border-radius: 8px;
}}
QLabel#BlockTitle {{ font-size: 14px; font-weight: 600; }}
QLabel#FieldLabel {{ color: {TEXT_DIM}; font-size: 11px; }}
QPlainTextEdit {{
    background: {BG_INPUT};
    border: 1px solid {LINE_HI};
    border-radius: 6px;
    padding: 7px 9px;
    selection-background-color: {accent};
}}
QPlainTextEdit:focus {{ border: 1px solid {accent}; }}
QPlainTextEdit#TranslationEdit {{ background: #191B20; color: #C9CDD4; }}
"""
