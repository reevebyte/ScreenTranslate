#pragma once

#include <algorithm>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace screentrans {

struct RectF {
    float x{};
    float y{};
    float width{};
    float height{};

    [[nodiscard]] float right() const noexcept { return x + width; }
    [[nodiscard]] float bottom() const noexcept { return y + height; }
    [[nodiscard]] float center_x() const noexcept { return x + width / 2.0F; }
    [[nodiscard]] bool empty() const noexcept { return width <= 0.0F || height <= 0.0F; }
};

struct PixelBuffer {
    int width{};
    int height{};
    int stride{};
    std::vector<std::uint8_t> bgra;

    [[nodiscard]] bool empty() const noexcept {
        return width <= 0 || height <= 0 || stride < width * 4 || bgra.empty();
    }

    [[nodiscard]] PixelBuffer crop(int left, int top, int crop_width, int crop_height) const {
        if (empty()) {
            throw std::runtime_error("cannot crop an empty image");
        }
        left = std::clamp(left, 0, width);
        top = std::clamp(top, 0, height);
        crop_width = std::clamp(crop_width, 0, width - left);
        crop_height = std::clamp(crop_height, 0, height - top);
        if (crop_width == 0 || crop_height == 0) {
            return {};
        }

        PixelBuffer out;
        out.width = crop_width;
        out.height = crop_height;
        out.stride = crop_width * 4;
        out.bgra.resize(static_cast<std::size_t>(out.stride) * out.height);
        for (int row = 0; row < crop_height; ++row) {
            const auto* source = bgra.data()
                + static_cast<std::size_t>(top + row) * stride
                + static_cast<std::size_t>(left) * 4;
            auto* destination = out.bgra.data() + static_cast<std::size_t>(row) * out.stride;
            std::copy_n(source, out.stride, destination);
        }
        return out;
    }
};

struct OcrWord {
    std::wstring text;
    RectF bounds;
    float confidence{-1.0F};
};

struct OcrLine {
    std::wstring text;
    RectF bounds;
    std::vector<OcrWord> words;
    float confidence{-1.0F};
    // Temporary grouping hint. OCR providers leave this unset; LayoutEngine
    // fills it before moving lines into paragraph blocks.
    float layout_column_right{-1.0F};
};

struct TextBlock {
    std::vector<OcrLine> lines;
    std::wstring edited_text;
    bool has_edited_text{};
    // Empty means automatic routing. A non-empty value is a user-selected
    // target language retained across retry operations.
    std::wstring forced_target_language;

    [[nodiscard]] RectF bounds() const;
    [[nodiscard]] float line_height() const;
    [[nodiscard]] std::wstring recognized_text() const;
    [[nodiscard]] std::wstring text() const {
        return has_edited_text ? edited_text : recognized_text();
    }
    [[nodiscard]] bool centered() const;
};

struct BlockTranslation {
    TextBlock block;
    std::wstring translated;
    std::wstring target_language;
};

}  // namespace screentrans
