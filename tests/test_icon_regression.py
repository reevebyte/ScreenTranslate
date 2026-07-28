from __future__ import annotations

import os
import unittest
from collections import deque

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QSize

from qt_helpers import application
from screentrans.ui.icon import make_icon


def _alpha_mask(size: int) -> list[list[bool]]:
    pixmap = make_icon("#28C76F", size).pixmap(QSize(size, size))
    image = pixmap.toImage()
    if image.width() != size or image.height() != size:
        raise AssertionError(
            f"requested {size}px icon, received {image.width()}x{image.height()}"
        )
    return [
        [image.pixelColor(x, y).alpha() > 0 for x in range(size)]
        for y in range(size)
    ]


def _opaque_components(mask: list[list[bool]]) -> int:
    height, width = len(mask), len(mask[0])
    seen: set[tuple[int, int]] = set()
    count = 0
    for y in range(height):
        for x in range(width):
            if not mask[y][x] or (x, y) in seen:
                continue
            count += 1
            queue = deque([(x, y)])
            seen.add((x, y))
            while queue:
                cx, cy = queue.popleft()
                for dx, dy in (
                    (-1, -1), (0, -1), (1, -1),
                    (-1, 0),             (1, 0),
                    (-1, 1),  (0, 1),    (1, 1),
                ):
                    nx, ny = cx + dx, cy + dy
                    if (
                        0 <= nx < width
                        and 0 <= ny < height
                        and mask[ny][nx]
                        and (nx, ny) not in seen
                    ):
                        seen.add((nx, ny))
                        queue.append((nx, ny))
    return count


def _enclosed_transparent_pixels(mask: list[list[bool]]) -> int:
    height, width = len(mask), len(mask[0])
    # Flood an extra transparent border. Any zero not reached from it is a hole.
    seen = {(0, 0)}
    queue = deque([(0, 0)])
    while queue:
        x, y = queue.popleft()
        for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            nx, ny = x + dx, y + dy
            if not (0 <= nx < width + 2 and 0 <= ny < height + 2):
                continue
            if (nx, ny) in seen:
                continue
            source_x, source_y = nx - 1, ny - 1
            if (
                0 <= source_x < width
                and 0 <= source_y < height
                and mask[source_y][source_x]
            ):
                continue
            seen.add((nx, ny))
            queue.append((nx, ny))
    return sum(
        1
        for y in range(height)
        for x in range(width)
        if not mask[y][x] and (x + 1, y + 1) not in seen
    )


class IconContourRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = application()

    def test_24_and_64_pixel_bubbles_are_connected_and_have_no_holes(self):
        for size in (24, 64):
            with self.subTest(size=size):
                mask = _alpha_mask(size)
                self.assertEqual(_opaque_components(mask), 1)
                self.assertEqual(_enclosed_transparent_pixels(mask), 0)


if __name__ == "__main__":
    unittest.main()
