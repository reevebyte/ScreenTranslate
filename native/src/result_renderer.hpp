#pragma once

#include "types.hpp"

#include <windows.h>

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

namespace screentrans {

struct PreparedTextLayer {
    std::size_t block_index{};
    RectF bounds;
    COLORREF background{};
    COLORREF foreground{};
    float line_height{};
    int font_pixels{};
    bool centered{};
    bool bold{};
};

struct PreparedResultRender {
    PixelBuffer background;
    PixelBuffer translated;
    std::vector<PreparedTextLayer> layers;
    std::wstring font_family;
};

PreparedResultRender prepare_result_render(
    const PixelBuffer& source,
    const std::vector<BlockTranslation>& translations,
    std::wstring_view requested_font_family,
    int minimum_font_pixels);

void result_renderer_self_test();

}  // namespace screentrans
