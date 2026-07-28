"""后台线程：识别 + 翻译。

放在独立线程里跑，界面才不会在等接口返回的时候卡住。
真正的绘制（QPainter）留给主线程做，避免跨线程用 Qt 绘图对象。
"""
from __future__ import annotations

import logging
from collections.abc import Callable, Sequence

from PySide6.QtCore import Signal

from . import langdetect, ocr, translators
from .background import DaemonWorker
from .error_logging import report_exception
from .layout import Block, group_lines
from .network import configured_secrets, redact_sensitive


def group_texts_by_target(
    texts: Sequence[str], targets: Sequence[str]
) -> dict[str, list[tuple[int, str]]]:
    """按目标语言分组，同时保留每段原来的位置。

    字典按目标语言第一次出现的顺序迭代，让真实接口调用和屏幕阅读顺序尽量一致；
    元组里的下标用于把译文放回原列表，不能依赖接口返回顺序跨组拼接。
    """
    if len(texts) != len(targets):
        raise ValueError("原文段数和目标语言段数不一致")

    groups: dict[str, list[tuple[int, str]]] = {}
    for index, (text, target) in enumerate(zip(texts, targets)):
        groups.setdefault(target, []).append((index, text))
    return groups


def translate_blocks(
    engine,
    blocks: Sequence[Block],
    zh_target: str = "en",
    *,
    targets: Sequence[str | None] | None = None,
    cancel_check: Callable[[], bool] | None = None,
) -> tuple[list[str], list[str]]:
    """逐块决定目标语言，按相同目标批量翻译，再按原顺序回填。

    ``targets`` 可由单段编辑等调用方显式传入；省略时沿用自动语言判断。
    返回值是 ``(translations, targets)``，两个列表都与 ``blocks`` 等长。
    """
    texts = [block.text for block in blocks]
    if targets is not None and len(targets) != len(texts):
        raise ValueError("文本块数和目标语言段数不一致")
    requested_targets = targets if targets is not None else [None] * len(texts)
    resolved_targets = [
        requested or langdetect.target_for(text, zh_target)[1]
        for text, requested in zip(texts, requested_targets)
    ]

    is_cancelled = cancel_check or (lambda: False)
    translated = [""] * len(texts)
    for target, indexed_texts in group_texts_by_target(texts, resolved_targets).items():
        if is_cancelled():
            raise translators.TranslateError("翻译已取消")
        batch = [text for _index, text in indexed_texts]
        batch_translated = engine.translate(batch, target, None)
        if is_cancelled():
            raise translators.TranslateError("翻译已取消")
        if len(batch_translated) != len(batch):
            raise translators.TranslateError(
                f"翻译接口返回了 {len(batch_translated)} 段，实际需要 {len(batch)} 段"
            )
        for (index, _text), value in zip(indexed_texts, batch_translated):
            translated[index] = value
    return translated, resolved_targets


class TranslateWorker(DaemonWorker):
    # blocks, 译文列表, 纯文本, 每个 block 的目标语言列表
    done = Signal(object, object, str, object)
    failed = Signal(str)

    def __init__(
        self,
        image,
        cfg,
        blocks: list[Block] | None = None,
        targets: Sequence[str | None] | None = None,
    ):
        """blocks 给了就跳过识别直接重翻。

        换个接口再试一次的时候，文字已经认过了，没必要再花几百毫秒重认一遍，
        而且重认还可能认出不一样的结果，让人分不清是接口的差别还是识别的差别。
        """
        super().__init__(thread_name="ScreenTranslate-Translate")
        # A retry/subset request already has OCR blocks and never reads the image.
        # Keeping another reference would pin a potentially full-screen RGB buffer
        # for the entire network timeout.
        self._image = image if blocks is None else None
        self._cfg = cfg
        self._blocks = blocks
        self._targets = targets
        self.cancelled = False

    def run(self):
        # A request can be superseded after its daemon thread was created but
        # before it actually got CPU time. Do not queue an obsolete OCR pass or
        # keep its screenshot alive behind the engine's global lock.
        if self.cancelled:
            self._image = None
            return
        if self._blocks is not None:
            blocks: list[Block] = self._blocks
        else:
            image = self._image
            ocr_secrets = configured_secrets(
                self._cfg.get("ocr.azure_vision", {}) or {}
            )
            try:
                lines = ocr.recognize(image, self._cfg)
            except Exception as exc:
                report_exception(logging.getLogger("screentrans.errors"), "worker.ocr", exc)
                if not self.cancelled:
                    self.failed.emit(f"识别失败：{redact_sensitive(exc, ocr_secrets)}")
                return
            finally:
                # Translation can spend up to the network timeout blocked in
                # requests. OCR is done, so the worker no longer needs the pixels.
                self._image = None
            if self.cancelled:
                return
            blocks = group_lines(lines)
        texts = [b.text for b in blocks]
        if not any(t.strip() for t in texts):
            if not self.cancelled:
                self.failed.emit("没有识别到文字")
            return

        provider = self._cfg.get("translator.provider", "microsoft")
        opts = self._cfg.get(f"translator.{provider}", {}) or {}
        engine = None
        try:
            engine = translators.build(provider, opts)
            engine.cancel_check = lambda: self.cancelled
            translated, targets = translate_blocks(
                engine,
                blocks,
                self._cfg.get("lang.zh_target", "en"),
                targets=self._targets,
                cancel_check=lambda: self.cancelled,
            )
        except Exception as exc:
            report_exception(logging.getLogger("screentrans.errors"), "worker.translate", exc)
            if not self.cancelled:
                self.failed.emit(
                    f"翻译失败：{translators.friendly(exc, configured_secrets(opts))}"
                )
            return
        finally:
            if engine is not None:
                close = getattr(engine, "close", None)
                if callable(close):
                    close()
        if self.cancelled:
            return

        self.done.emit(blocks, translated, "\n".join(t for t in translated if t), targets)
