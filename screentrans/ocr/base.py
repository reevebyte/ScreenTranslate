"""OCR 结果的通用数据结构。坐标一律是相对截图左上角的物理像素。"""
from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Word:
    text: str
    x: float
    y: float
    w: float
    h: float
    confidence: float | None = None


@dataclass
class Line:
    text: str
    x: float
    y: float
    w: float
    h: float
    words: list[Word] = field(default_factory=list)
    confidence: float | None = None

    @property
    def right(self) -> float:
        return self.x + self.w

    @property
    def bottom(self) -> float:
        return self.y + self.h

    @property
    def cx(self) -> float:
        return self.x + self.w / 2


class OcrError(RuntimeError):
    pass
