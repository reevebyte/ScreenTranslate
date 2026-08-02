#include "result_renderer.hpp"

#include "util.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cwctype>
#include <limits>
#include <string>
#include <string_view>
#include <vector>

namespace screentrans {

namespace {

constexpr int text_pad_x = 2;
constexpr int text_pad_y = 1;
constexpr double maximum_height_growth = 2.5;
constexpr double maximum_width_growth = 2.4;
constexpr double shrink_floor = 0.8;

struct SampledColors {
    COLORREF background{};
    COLORREF foreground{};
};

double luminance(COLORREF color) {
    return 0.2126 * GetRValue(color) + 0.7152 * GetGValue(color) + 0.0722 * GetBValue(color);
}

double linear_channel(BYTE value) {
    const double channel = static_cast<double>(value) / 255.0;
    return channel <= 0.03928 ? channel / 12.92
                             : std::pow((channel + 0.055) / 1.055, 2.4);
}

double relative_luminance(COLORREF color) {
    return 0.2126 * linear_channel(GetRValue(color))
         + 0.7152 * linear_channel(GetGValue(color))
         + 0.0722 * linear_channel(GetBValue(color));
}

double contrast_ratio(COLORREF left, COLORREF right) {
    const double left_luminance = relative_luminance(left);
    const double right_luminance = relative_luminance(right);
    const double lighter = std::max(left_luminance, right_luminance);
    const double darker = std::min(left_luminance, right_luminance);
    return (lighter + 0.05) / (darker + 0.05);
}

RECT pixel_rect(const RectF& bounds, const PixelBuffer& image) {
    const int left = std::clamp(static_cast<int>(std::floor(bounds.x)) - text_pad_x,
                                0, image.width);
    const int top = std::clamp(static_cast<int>(std::floor(bounds.y)) - text_pad_y,
                               0, image.height);
    const int right = std::clamp(static_cast<int>(std::ceil(bounds.right())) + text_pad_x,
                                 left, image.width);
    const int bottom = std::clamp(static_cast<int>(std::ceil(bounds.bottom())) + text_pad_y,
                                  top, image.height);
    return {left, top, right, bottom};
}

RectF rect_f(const RECT& rect) {
    return {static_cast<float>(rect.left), static_cast<float>(rect.top),
            static_cast<float>(rect.right - rect.left),
            static_cast<float>(rect.bottom - rect.top)};
}

bool rects_overlap_horizontally(const RECT& left, const RECT& right) noexcept {
    return left.right > right.left && right.right > left.left;
}

bool rects_overlap_vertically(const RECT& left, const RECT& right) noexcept {
    return left.bottom > right.top && right.bottom > left.top;
}

int grow_limit_down(const RECT& rect, const std::vector<RECT>& others, int image_height) {
    int limit = image_height;
    for (const auto& other : others) {
        if (other.top <= rect.top || !rects_overlap_horizontally(rect, other)) continue;
        limit = std::min(limit, static_cast<int>(other.top));
    }
    return std::max(static_cast<int>(rect.bottom), limit);
}

int grow_limit_up(const RECT& rect, const std::vector<RECT>& others) {
    int limit = 0;
    for (const auto& other : others) {
        if (other.bottom >= rect.bottom || !rects_overlap_horizontally(rect, other)) continue;
        limit = std::max(limit, static_cast<int>(other.bottom));
    }
    return std::min(static_cast<int>(rect.top), limit);
}

int grow_limit_right(const RECT& rect, const std::vector<RECT>& others, int image_width) {
    int limit = image_width;
    for (const auto& other : others) {
        if (other.left <= rect.left || !rects_overlap_vertically(rect, other)) continue;
        limit = std::min(limit, static_cast<int>(other.left));
    }
    return std::max(static_cast<int>(rect.right), limit);
}

int background_run(const PixelBuffer& image, const RECT& rect, COLORREF background,
                   int limit, bool down) {
    const int left = std::clamp(static_cast<int>(rect.left), 0, image.width);
    const int right = std::clamp(static_cast<int>(rect.right), left, image.width);
    if (right - left < 4) return limit;
    const int red = GetRValue(background);
    const int green = GetGValue(background);
    const int blue = GetBValue(background);
    const auto row_matches = [&](int y) {
        int matched = 0;
        const auto* row = image.bgra.data() + static_cast<std::size_t>(y) * image.stride;
        for (int x = left; x < right; ++x) {
            const auto* pixel = row + static_cast<std::size_t>(x) * 4;
            if (std::abs(static_cast<int>(pixel[2]) - red) <= 26
                && std::abs(static_cast<int>(pixel[1]) - green) <= 26
                && std::abs(static_cast<int>(pixel[0]) - blue) <= 26) {
                ++matched;
            }
        }
        return matched * 4 >= (right - left) * 3;
    };

    if (down) {
        const int end = std::clamp(limit, static_cast<int>(rect.bottom), image.height);
        for (int y = std::clamp(static_cast<int>(rect.bottom), 0, image.height);
             y < end; ++y) {
            if (!row_matches(y)) return y;
        }
        return end;
    }
    const int begin = std::clamp(limit, 0, static_cast<int>(rect.top));
    for (int y = std::clamp(static_cast<int>(rect.top), 0, image.height) - 1;
         y >= begin; --y) {
        if (!row_matches(y)) return y + 1;
    }
    return begin;
}

bool in_text_flow(const RECT& rect, float line_height,
                  const std::vector<RECT>& others,
                  const std::vector<float>& other_line_heights) {
    for (std::size_t index = 0; index < others.size(); ++index) {
        const auto& other = others[index];
        if (!rects_overlap_horizontally(rect, other)) continue;
        if (index >= other_line_heights.size()
            || std::abs(other_line_heights[index] - line_height) > line_height * 0.18F) {
            continue;
        }
        const int gap = other.top > rect.top
            ? static_cast<int>(other.top - rect.bottom)
            : static_cast<int>(rect.top - other.bottom);
        if (gap >= 0 && gap <= line_height * 3.0F) return true;
    }
    return false;
}

int CALLBACK count_font(const LOGFONTW*, const TEXTMETRICW*, DWORD, LPARAM parameter) {
    *reinterpret_cast<bool*>(parameter) = true;
    return 0;
}

bool font_exists(HDC dc, std::wstring_view family) {
    if (family.empty() || family.size() >= LF_FACESIZE) return false;
    LOGFONTW request{};
    request.lfCharSet = DEFAULT_CHARSET;
    std::copy(family.begin(), family.end(), request.lfFaceName);
    bool found = false;
    EnumFontFamiliesExW(dc, &request, &count_font, reinterpret_cast<LPARAM>(&found), 0);
    return found;
}

bool is_east_asian_character(wchar_t value) noexcept {
    const unsigned code = static_cast<unsigned>(value);
    return (code >= 0x2E80 && code <= 0x9FFF)
        || (code >= 0xAC00 && code <= 0xD7AF)
        || (code >= 0xF900 && code <= 0xFAFF);
}

bool font_covers_text(HDC dc, std::wstring_view family,
                      const std::vector<BlockTranslation>& translations) {
    HFONT font = CreateFontW(-16, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                             DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                             CLEARTYPE_QUALITY, DEFAULT_PITCH, std::wstring(family).c_str());
    if (!font) return false;
    const HGDIOBJ previous = SelectObject(dc, font);
    bool covered = true;
    for (const auto& block : translations) {
        for (const wchar_t character : block.translated) {
            if (!is_east_asian_character(character)) continue;
            WORD glyph = 0xFFFF;
            if (GetGlyphIndicesW(dc, &character, 1, &glyph,
                                 GGI_MARK_NONEXISTING_GLYPHS) == GDI_ERROR
                || glyph == 0xFFFF) {
                covered = false;
                break;
            }
        }
        if (!covered) break;
    }
    SelectObject(dc, previous);
    DeleteObject(font);
    return covered;
}

std::wstring resolve_font_family(HDC dc, std::wstring_view requested,
                                 const std::vector<BlockTranslation>& translations) {
    constexpr std::array<std::wstring_view, 6> fallbacks{
        L"Microsoft YaHei UI", L"微软雅黑", L"Microsoft YaHei",
        L"Noto Sans SC", L"SimHei", L"SimSun",
    };
    const auto usable = [&](std::wstring_view family) {
        return font_exists(dc, family) && font_covers_text(dc, family, translations);
    };
    if (usable(requested)) return std::wstring(requested);
    for (const auto family : fallbacks) {
        if (usable(family)) return std::wstring(family);
    }
    return requested.empty() ? std::wstring(L"Microsoft YaHei UI")
                             : std::wstring(requested);
}

struct FontFit {
    int pixels{};
    SIZE needed{};
    int line_box{};
};

SIZE measure_wrapped_text(HDC dc, HFONT font, std::wstring_view text, int width,
                          bool centered) {
    const HGDIOBJ previous = SelectObject(dc, font);
    RECT needed{0, 0, std::max(1, width), 100000};
    DrawTextW(dc, text.data(), static_cast<int>(text.size()), &needed,
              DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | DT_EDITCONTROL
                  | (centered ? DT_CENTER : DT_LEFT));
    SelectObject(dc, previous);
    return {std::max(1L, needed.right - needed.left),
            std::max(1L, needed.bottom - needed.top)};
}

FontFit fit_font(HDC dc, std::wstring_view text, std::wstring_view family,
                 int width, int height, int start_pixels, int minimum_pixels,
                 bool centered, bool bold) {
    FontFit result{std::max(1, minimum_pixels), {1, std::max(1, height)}, 1};
    const int first = std::clamp(std::max(start_pixels, minimum_pixels),
                                 minimum_pixels, 96);
    for (int pixels = first; pixels >= minimum_pixels; --pixels) {
        HFONT font = CreateFontW(-pixels, 0, 0, 0, bold ? FW_SEMIBOLD : FW_NORMAL,
                                 FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                                 OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                                 CLEARTYPE_QUALITY, DEFAULT_PITCH,
                                 std::wstring(family).c_str());
        if (!font) continue;
        const HGDIOBJ previous = SelectObject(dc, font);
        TEXTMETRICW metrics{};
        GetTextMetricsW(dc, &metrics);
        SelectObject(dc, previous);
        const SIZE needed = measure_wrapped_text(dc, font, text, width, centered);
        DeleteObject(font);
        result = {pixels, needed, std::max(1L, metrics.tmHeight)};
        if (needed.cx <= width && needed.cy <= height) return result;
    }
    return result;
}

int tight_ink_height(HDC dc, std::wstring_view text, std::wstring_view family,
                     int pixels, bool bold) {
    HFONT font = CreateFontW(-std::max(1, pixels), 0, 0, 0,
                             bold ? FW_SEMIBOLD : FW_NORMAL,
                             FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                             OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                             CLEARTYPE_QUALITY, DEFAULT_PITCH,
                             std::wstring(family).c_str());
    if (!font) return 0;

    const HGDIOBJ previous = SelectObject(dc, font);
    MAT2 transform{};
    transform.eM11.value = 1;
    transform.eM22.value = 1;
    int highest = 0;
    int lowest = 0;
    bool measured = false;
    for (const wchar_t character : text) {
        if (character == L'\r' || character == L'\n' || character == L'\t'
            || character == L' ') {
            continue;
        }
        GLYPHMETRICS glyph{};
        if (GetGlyphOutlineW(dc, character, GGO_METRICS, &glyph, 0, nullptr,
                             &transform) == GDI_ERROR
            || glyph.gmBlackBoxY == 0) {
            continue;
        }
        const int top = glyph.gmptGlyphOrigin.y;
        const int bottom = top - static_cast<int>(glyph.gmBlackBoxY);
        highest = measured ? std::max(highest, top) : top;
        lowest = measured ? std::min(lowest, bottom) : bottom;
        measured = true;
    }
    SelectObject(dc, previous);
    DeleteObject(font);
    return measured ? std::max(1, highest - lowest) : 0;
}

int size_for_ink(HDC dc, std::wstring_view text, std::wstring_view family,
                 bool bold, float target_height, int minimum_pixels,
                 int maximum_pixels) {
    minimum_pixels = std::max(1, minimum_pixels);
    maximum_pixels = std::max(minimum_pixels, maximum_pixels);
    int best = minimum_pixels;
    float best_difference = std::numeric_limits<float>::max();
    for (int pixels = minimum_pixels; pixels <= maximum_pixels; ++pixels) {
        const int height = tight_ink_height(dc, text, family, pixels, bold);
        if (height <= 0) continue;
        const float difference = std::abs(static_cast<float>(height) - target_height);
        if (difference < best_difference) {
            best = pixels;
            best_difference = difference;
        }
        if (static_cast<float>(height) > target_height * 1.4F) break;
    }
    return best;
}

BYTE histogram_median(const std::array<std::uint32_t, 256>& histogram,
                      std::uint64_t count) {
    const std::uint64_t rank = count / 2;
    std::uint64_t seen = 0;
    for (std::size_t value = 0; value < histogram.size(); ++value) {
        seen += histogram[value];
        if (seen > rank) return static_cast<BYTE>(value);
    }
    return 0;
}

SampledColors sample_colors(const PixelBuffer& image, const RECT& rect, float line_height) {
    if (image.empty() || rect.right <= rect.left || rect.bottom <= rect.top) {
        return {RGB(255, 255, 255), RGB(17, 17, 17)};
    }

    const int padding = std::max(2, static_cast<int>(std::lround(
        std::max(1.0F, line_height) * 0.35F)));
    const int inner_left = static_cast<int>(rect.left);
    const int inner_top = static_cast<int>(rect.top);
    const int inner_right = static_cast<int>(rect.right);
    const int inner_bottom = static_cast<int>(rect.bottom);
    const int outer_left = std::max(0, inner_left - padding);
    const int outer_top = std::max(0, inner_top - padding);
    const int outer_right = std::min(image.width, inner_right + padding);
    const int outer_bottom = std::min(image.height, inner_bottom + padding);
    std::array<std::array<std::uint32_t, 256>, 3> histograms{};
    std::uint64_t ring_count = 0;

    for (int y = outer_top; y < outer_bottom; ++y) {
        const auto* row = image.bgra.data() + static_cast<std::size_t>(y) * image.stride;
        for (int x = outer_left; x < outer_right; ++x) {
            if (x >= inner_left && x < inner_right && y >= inner_top && y < inner_bottom) continue;
            const auto* pixel = row + static_cast<std::size_t>(x) * 4;
            ++histograms[0][pixel[2]];
            ++histograms[1][pixel[1]];
            ++histograms[2][pixel[0]];
            ++ring_count;
        }
    }

    if (ring_count < 24) {
        histograms = {};
        ring_count = 0;
        for (int y = rect.top; y < rect.bottom; ++y) {
            const auto* row = image.bgra.data() + static_cast<std::size_t>(y) * image.stride;
            for (int x = rect.left; x < rect.right; ++x) {
                const auto* pixel = row + static_cast<std::size_t>(x) * 4;
                ++histograms[0][pixel[2]];
                ++histograms[1][pixel[1]];
                ++histograms[2][pixel[0]];
                ++ring_count;
            }
        }
    }

    const COLORREF background = RGB(histogram_median(histograms[0], ring_count),
                                    histogram_median(histograms[1], ring_count),
                                    histogram_median(histograms[2], ring_count));
    struct ForegroundPixel {
        std::uint32_t distance{};
        BYTE red{};
        BYTE green{};
        BYTE blue{};
    };
    std::vector<ForegroundPixel> candidates;
    candidates.reserve(static_cast<std::size_t>(rect.right - rect.left)
                       * static_cast<std::size_t>(rect.bottom - rect.top) / 4);
    const int background_red = GetRValue(background);
    const int background_green = GetGValue(background);
    const int background_blue = GetBValue(background);
    for (int y = rect.top; y < rect.bottom; ++y) {
        const auto* row = image.bgra.data() + static_cast<std::size_t>(y) * image.stride;
        for (int x = rect.left; x < rect.right; ++x) {
            const auto* pixel = row + static_cast<std::size_t>(x) * 4;
            const int red_delta = static_cast<int>(pixel[2]) - background_red;
            const int green_delta = static_cast<int>(pixel[1]) - background_green;
            const int blue_delta = static_cast<int>(pixel[0]) - background_blue;
            const auto distance = static_cast<std::uint32_t>(red_delta * red_delta
                               + green_delta * green_delta + blue_delta * blue_delta);
            if (distance > 60U * 60U) {
                candidates.push_back({distance, pixel[2], pixel[1], pixel[0]});
            }
        }
    }

    COLORREF foreground = luminance(background) > 127.0
        ? RGB(17, 17, 17) : RGB(240, 240, 240);
    const auto pixel_count = static_cast<std::size_t>(rect.right - rect.left)
                           * static_cast<std::size_t>(rect.bottom - rect.top);
    if (candidates.size() >= std::max<std::size_t>(8, pixel_count / 100)) {
        const std::size_t cutoff_index = candidates.size() * 3 / 5;
        std::nth_element(candidates.begin(), candidates.begin() + cutoff_index,
                         candidates.end(), [](const auto& left, const auto& right) {
                             return left.distance < right.distance;
                         });
        const auto cutoff = candidates[cutoff_index].distance;
        std::uint64_t red = 0, green = 0, blue = 0, count = 0;
        for (const auto& candidate : candidates) {
            if (candidate.distance < cutoff) continue;
            red += candidate.red;
            green += candidate.green;
            blue += candidate.blue;
            ++count;
        }
        if (count) foreground = RGB(red / count, green / count, blue / count);
    }
    if (contrast_ratio(background, foreground) < 3.0) {
        foreground = relative_luminance(background) > 0.45
            ? RGB(17, 17, 17) : RGB(240, 240, 240);
    }
    return {background, foreground};
}

void repair_text_region(PixelBuffer& destination, const PixelBuffer& source,
                        const RECT& rect, bool dark_text, float line_height) {
    const int width = rect.right - rect.left;
    const int height = rect.bottom - rect.top;
    if (width < 3 || height < 3) return;
    const std::size_t pixel_count = static_cast<std::size_t>(width)
                                  * static_cast<std::size_t>(height);
    if (pixel_count > 2'000'000) {
        // Bound transient memory when a malformed OCR box covers almost the desktop.
        for (int y = 0; y < height; ++y) {
            auto* destination_row = destination.bgra.data()
                + static_cast<std::size_t>(rect.top + y) * destination.stride
                + static_cast<std::size_t>(rect.left) * 4;
            for (int x = 0; x < width; ++x) {
                for (int channel = 0; channel < 3; ++channel) {
                    const auto at = [&](int source_x, int source_y) {
                        return source.bgra[static_cast<std::size_t>(rect.top + source_y)
                                                 * source.stride
                                           + static_cast<std::size_t>(rect.left + source_x) * 4
                                           + channel];
                    };
                    const int horizontal = (at(0, y) * (width - 1 - x)
                                          + at(width - 1, y) * x) / (width - 1);
                    const int vertical = (at(x, 0) * (height - 1 - y)
                                        + at(x, height - 1) * y) / (height - 1);
                    destination_row[static_cast<std::size_t>(x) * 4 + channel]
                        = static_cast<BYTE>((horizontal + vertical) / 2);
                }
                destination_row[static_cast<std::size_t>(x) * 4 + 3] = 255;
            }
        }
        return;
    }

    const std::size_t patch_stride = static_cast<std::size_t>(width) * 4;
    std::vector<std::uint8_t> patch(patch_stride * static_cast<std::size_t>(height));
    std::vector<std::uint8_t> scratch(patch.size());
    for (int y = 0; y < height; ++y) {
        const auto* source_row = source.bgra.data()
            + static_cast<std::size_t>(rect.top + y) * source.stride
            + static_cast<std::size_t>(rect.left) * 4;
        std::copy_n(source_row, patch_stride,
                    patch.data() + static_cast<std::size_t>(y) * patch_stride);
    }

    int filter_size = std::clamp(static_cast<int>(std::lround(
        std::max(1.0F, line_height) * 0.34F)), 3, 15);
    if ((filter_size & 1) == 0) ++filter_size;
    const auto choose = [dark_text](BYTE left, BYTE middle, BYTE right) {
        return dark_text ? std::max({left, middle, right})
                         : std::min({left, middle, right});
    };
    for (int pass = 0; pass < filter_size / 2; ++pass) {
        for (int y = 0; y < height; ++y) {
            for (int x = 0; x < width; ++x) {
                const int previous = std::max(0, x - 1);
                const int next = std::min(width - 1, x + 1);
                for (int channel = 0; channel < 3; ++channel) {
                    scratch[static_cast<std::size_t>(y) * patch_stride
                          + static_cast<std::size_t>(x) * 4 + channel] = choose(
                        patch[static_cast<std::size_t>(y) * patch_stride
                            + static_cast<std::size_t>(previous) * 4 + channel],
                        patch[static_cast<std::size_t>(y) * patch_stride
                            + static_cast<std::size_t>(x) * 4 + channel],
                        patch[static_cast<std::size_t>(y) * patch_stride
                            + static_cast<std::size_t>(next) * 4 + channel]);
                }
                scratch[static_cast<std::size_t>(y) * patch_stride
                      + static_cast<std::size_t>(x) * 4 + 3] = 255;
            }
        }
        for (int y = 0; y < height; ++y) {
            const int previous = std::max(0, y - 1);
            const int next = std::min(height - 1, y + 1);
            for (int x = 0; x < width; ++x) {
                for (int channel = 0; channel < 3; ++channel) {
                    patch[static_cast<std::size_t>(y) * patch_stride
                        + static_cast<std::size_t>(x) * 4 + channel] = choose(
                        scratch[static_cast<std::size_t>(previous) * patch_stride
                              + static_cast<std::size_t>(x) * 4 + channel],
                        scratch[static_cast<std::size_t>(y) * patch_stride
                              + static_cast<std::size_t>(x) * 4 + channel],
                        scratch[static_cast<std::size_t>(next) * patch_stride
                              + static_cast<std::size_t>(x) * 4 + channel]);
                }
                patch[static_cast<std::size_t>(y) * patch_stride
                    + static_cast<std::size_t>(x) * 4 + 3] = 255;
            }
        }
    }

    for (int y = 0; y < height; ++y) {
        for (int x = 0; x < width; ++x) {
            const int previous = std::max(0, x - 1);
            const int next = std::min(width - 1, x + 1);
            for (int channel = 0; channel < 3; ++channel) {
                scratch[static_cast<std::size_t>(y) * patch_stride
                      + static_cast<std::size_t>(x) * 4 + channel] = static_cast<BYTE>((
                    patch[static_cast<std::size_t>(y) * patch_stride
                          + static_cast<std::size_t>(previous) * 4 + channel]
                  + patch[static_cast<std::size_t>(y) * patch_stride
                          + static_cast<std::size_t>(x) * 4 + channel]
                  + patch[static_cast<std::size_t>(y) * patch_stride
                          + static_cast<std::size_t>(next) * 4 + channel]) / 3);
            }
            scratch[static_cast<std::size_t>(y) * patch_stride
                  + static_cast<std::size_t>(x) * 4 + 3] = 255;
        }
    }
    for (int y = 0; y < height; ++y) {
        auto* destination_row = destination.bgra.data()
            + static_cast<std::size_t>(rect.top + y) * destination.stride
            + static_cast<std::size_t>(rect.left) * 4;
        for (int x = 0; x < width; ++x) {
            const int previous = std::max(0, y - 1);
            const int next = std::min(height - 1, y + 1);
            for (int channel = 0; channel < 3; ++channel) {
                destination_row[static_cast<std::size_t>(x) * 4 + channel] = static_cast<BYTE>((
                    scratch[static_cast<std::size_t>(previous) * patch_stride
                          + static_cast<std::size_t>(x) * 4 + channel]
                  + scratch[static_cast<std::size_t>(y) * patch_stride
                          + static_cast<std::size_t>(x) * 4 + channel]
                  + scratch[static_cast<std::size_t>(next) * patch_stride
                          + static_cast<std::size_t>(x) * 4 + channel]) / 3);
            }
            destination_row[static_cast<std::size_t>(x) * 4 + 3] = 255;
        }
    }
}

bool draw_translated_text(PixelBuffer& image,
                          const std::vector<PreparedTextLayer>& layers,
                          const std::vector<BlockTranslation>& translations,
                          std::wstring_view font_family) {
    if (image.empty() || layers.empty()) return true;
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = image.width;
    info.bmiHeader.biHeight = -image.height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    void* bits = nullptr;
    HBITMAP bitmap = CreateDIBSection(nullptr, &info, DIB_RGB_COLORS,
                                      &bits, nullptr, 0);
    HDC dc = bitmap ? CreateCompatibleDC(nullptr) : nullptr;
    if (!bitmap || !dc || !bits) {
        if (dc) DeleteDC(dc);
        if (bitmap) DeleteObject(bitmap);
        return false;
    }
    const HGDIOBJ original_bitmap = SelectObject(dc, bitmap);
    if (!original_bitmap || original_bitmap == HGDI_ERROR) {
        DeleteDC(dc);
        DeleteObject(bitmap);
        return false;
    }
    const int dib_stride = image.width * 4;
    auto* destination = static_cast<std::uint8_t*>(bits);
    for (int row = 0; row < image.height; ++row) {
        std::copy_n(image.bgra.data() + static_cast<std::size_t>(row) * image.stride,
                    dib_stride,
                    destination + static_cast<std::size_t>(row) * dib_stride);
    }

    SetBkMode(dc, TRANSPARENT);
    for (const auto& layer : layers) {
        if (layer.block_index >= translations.size()) continue;
        const auto text = trim(translations[layer.block_index].translated);
        if (text.empty()) continue;
        HFONT font = CreateFontW(
            -std::max(1, layer.font_pixels), 0, 0, 0,
            layer.bold ? FW_SEMIBOLD : FW_NORMAL,
            FALSE, FALSE, FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH,
            std::wstring(font_family).c_str());
        if (!font) continue;
        const HGDIOBJ previous_font = SelectObject(dc, font);
        SetTextColor(dc, layer.foreground);
        RECT bounds{
            static_cast<LONG>(std::lround(layer.bounds.x)),
            static_cast<LONG>(std::lround(layer.bounds.y)),
            static_cast<LONG>(std::lround(layer.bounds.right())),
            static_cast<LONG>(std::lround(layer.bounds.bottom())),
        };
        DrawTextW(dc, text.data(), static_cast<int>(text.size()), &bounds,
                  DT_TOP | DT_WORDBREAK | DT_NOPREFIX | DT_EDITCONTROL |
                      (layer.centered ? DT_CENTER : DT_LEFT));
        SelectObject(dc, previous_font);
        DeleteObject(font);
    }
    GdiFlush();
    for (int row = 0; row < image.height; ++row) {
        std::copy_n(destination + static_cast<std::size_t>(row) * dib_stride,
                    dib_stride,
                    image.bgra.data() + static_cast<std::size_t>(row) * image.stride);
    }
    for (std::size_t index = 3; index < image.bgra.size(); index += 4) {
        image.bgra[index] = 0xFF;
    }
    SelectObject(dc, original_bitmap);
    DeleteDC(dc);
    DeleteObject(bitmap);
    return true;
}

}  // namespace

PreparedResultRender prepare_result_render(
    const PixelBuffer& source,
    const std::vector<BlockTranslation>& translations,
    std::wstring_view requested_font_family,
    int minimum_font_pixels) {
    PreparedResultRender output;
    if (source.empty()) return output;
    output.background = source;
    output.layers.reserve(translations.size());
    minimum_font_pixels = std::clamp(minimum_font_pixels, 6, 72);

    HDC metrics_dc = CreateCompatibleDC(nullptr);
    if (!metrics_dc) return output;
    output.font_family = resolve_font_family(
        metrics_dc, requested_font_family, translations);

    std::vector<RECT> original_rects;
    std::vector<float> line_heights;
    original_rects.reserve(translations.size());
    line_heights.reserve(translations.size());
    for (const auto& translated : translations) {
        original_rects.push_back(pixel_rect(translated.block.bounds(), source));
        line_heights.push_back(std::max(1.0F, translated.block.line_height()));
    }

    std::vector<RECT> placed;
    std::vector<float> placed_heights;
    placed.reserve(translations.size());
    placed_heights.reserve(translations.size());
    for (std::size_t index = 0; index < translations.size(); ++index) {
        const auto& translated = translations[index];
        const RECT original = original_rects[index];
        const float line_height = line_heights[index];
        if (trim(translated.translated).empty()
            || trim(translated.translated) == trim(translated.block.text())) {
            placed.push_back(original);
            placed_heights.push_back(line_height);
            continue;
        }
        const int original_width = static_cast<int>(original.right - original.left);
        const int original_height = static_cast<int>(original.bottom - original.top);
        if (original_width < 3 || original_height < 3) {
            placed.push_back(original);
            placed_heights.push_back(line_height);
            continue;
        }
        const auto colors = sample_colors(source, original, line_height);
        const bool bold = translated.block.lines.size() == 1 && line_height >= 22.0F;
        const bool centered = translated.block.centered();
        const auto text = trim(translated.translated);
        const int maximum_start_pixels = std::max(
            minimum_font_pixels, static_cast<int>(line_height * 2.0F) + 4);
        const int start_pixels = size_for_ink(
            metrics_dc, text, output.font_family, bold, line_height,
            minimum_font_pixels, maximum_start_pixels);

        std::vector<RECT> others = placed;
        std::vector<float> other_heights = placed_heights;
        others.insert(others.end(), original_rects.begin() + static_cast<std::ptrdiff_t>(index + 1),
                      original_rects.end());
        other_heights.insert(other_heights.end(),
                             line_heights.begin() + static_cast<std::ptrdiff_t>(index + 1),
                             line_heights.end());

        int maximum_width = std::min(
            grow_limit_right(original, others, source.width) - static_cast<int>(original.left),
            static_cast<int>(std::lround(original_width * maximum_width_growth)));
        maximum_width = std::max(original_width, maximum_width);

        int down_to = grow_limit_down(original, others, source.height);
        down_to = std::min(down_to,
                           background_run(source, original, colors.background, down_to, true));
        int up_to = grow_limit_up(original, others);
        up_to = std::max(up_to,
                         background_run(source, original, colors.background, up_to, false));
        int maximum_height = std::min(
            down_to - up_to,
            static_cast<int>(std::lround(original_height * maximum_height_growth)));
        maximum_height = std::max(original_height, maximum_height);

        const FontFit probe = fit_font(
            metrics_dc, text, output.font_family, maximum_width, 100000,
            start_pixels, minimum_font_pixels, centered, bold);
        const bool flowing = translated.block.lines.size() >= 2
            || in_text_flow(original, line_height, others, other_heights);
        const int fit_height = flowing
            ? maximum_height
            : std::max(original_height, std::min(maximum_height, probe.line_box));
        FontFit fitted = fit_font(
            metrics_dc, text, output.font_family, maximum_width, fit_height,
            start_pixels, minimum_font_pixels, centered, bold);
        if (fitted.pixels < static_cast<int>(std::floor(start_pixels * shrink_floor))
            && maximum_height > fit_height) {
            fitted = fit_font(metrics_dc, text, output.font_family,
                              maximum_width, maximum_height, start_pixels,
                              minimum_font_pixels, centered, bold);
        }

        const int final_width = std::max(
            original_width,
            std::min(maximum_width,
                     static_cast<int>(fitted.needed.cx) + text_pad_x * 2));
        const int final_height = std::max(
            original_height,
            std::min(maximum_height, static_cast<int>(fitted.needed.cy)));
        int final_top = static_cast<int>(original.top);
        const int extra_height = final_height - original_height;
        if (extra_height > 1) {
            const int centered_top = static_cast<int>(original.top) - extra_height / 2;
            final_top = std::clamp(centered_top, up_to, down_to - final_height);
        }
        RECT final_rect{
            original.left,
            final_top,
            std::min(static_cast<LONG>(source.width), original.left + final_width),
            std::min(static_cast<LONG>(source.height), static_cast<LONG>(final_top + final_height)),
        };
        if (final_rect.right <= final_rect.left || final_rect.bottom <= final_rect.top) {
            placed.push_back(original);
            placed_heights.push_back(line_height);
            continue;
        }
        placed.push_back(final_rect);
        placed_heights.push_back(line_height);
        repair_text_region(output.background, source, final_rect,
                           luminance(colors.foreground) < luminance(colors.background),
                           line_height);
        output.layers.push_back({
            index,
            rect_f(final_rect),
            colors.background,
            colors.foreground,
            line_height,
            fitted.pixels,
            centered,
            bold,
        });
    }
    output.translated = output.background;
    if (!draw_translated_text(output.translated, output.layers,
                              translations, output.font_family)) {
        output.translated = output.background;
    }
    DeleteDC(metrics_dc);
    return output;
}

void result_renderer_self_test() {
    PixelBuffer source;
    source.width = 180;
    source.height = 72;
    source.stride = source.width * 4;
    source.bgra.assign(static_cast<std::size_t>(source.stride) * source.height, 0xFF);

    OcrLine line;
    line.text = L"Original";
    line.bounds = {20.0F, 18.0F, 100.0F, 26.0F};
    line.confidence = 1.0F;

    BlockTranslation translation;
    translation.block.lines.push_back(std::move(line));
    translation.translated = L"Translated";
    translation.target_language = L"en";

    const auto prepared = prepare_result_render(
        source, {translation}, L"Segoe UI", 8);
    if (prepared.background.empty() || prepared.translated.empty()
        || prepared.layers.size() != 1
        || prepared.background.width != prepared.translated.width
        || prepared.background.height != prepared.translated.height) {
        throw AppError("result renderer did not prepare a complete translated surface");
    }

    const RECT bounds = pixel_rect(prepared.layers.front().bounds, prepared.translated);
    bool contains_text_pixels = false;
    for (int y = bounds.top; y < bounds.bottom && !contains_text_pixels; ++y) {
        const std::size_t row = static_cast<std::size_t>(y) * prepared.translated.stride;
        for (int x = bounds.left; x < bounds.right; ++x) {
            const std::size_t pixel = row + static_cast<std::size_t>(x) * 4;
            for (std::size_t channel = 0; channel < 3; ++channel) {
                if (prepared.translated.bgra[pixel + channel]
                    != prepared.background.bgra[pixel + channel]) {
                    contains_text_pixels = true;
                    break;
                }
            }
            if (contains_text_pixels) break;
        }
    }
    if (!contains_text_pixels) {
        throw AppError("result renderer did not bake translated text into the parent surface");
    }
}

}  // namespace screentrans
