#include "layout.hpp"

#include "language.hpp"

#include <algorithm>
#include <cmath>
#include <cwctype>
#include <iterator>
#include <limits>
#include <numeric>
#include <optional>

namespace screentrans {

namespace {

float median(std::vector<float> values) {
    if (values.empty()) {
        return 0.0F;
    }
    const auto middle = values.begin() + static_cast<std::ptrdiff_t>(values.size() / 2);
    std::nth_element(values.begin(), middle, values.end());
    if (values.size() % 2 != 0) {
        return *middle;
    }
    const float upper = *middle;
    const float lower = *std::max_element(values.begin(), middle);
    return (lower + upper) / 2.0F;
}

float variance(const std::vector<float>& values, std::size_t begin, std::size_t end) {
    if (end <= begin + 1) {
        return 0.0F;
    }
    const float mean = std::accumulate(values.begin() + begin, values.begin() + end, 0.0F) /
                       static_cast<float>(end - begin);
    float sum = 0.0F;
    for (std::size_t index = begin; index < end; ++index) {
        const float difference = values[index] - mean;
        sum += difference * difference;
    }
    return sum / static_cast<float>(end - begin);
}

std::optional<float> observed_pitch(const TextBlock& block) {
    if (block.lines.size() < 2) {
        return std::nullopt;
    }
    float result = std::numeric_limits<float>::max();
    for (std::size_t index = 0; index + 1 < block.lines.size(); ++index) {
        result = std::min(result, block.lines[index + 1].bounds.y - block.lines[index].bounds.y);
    }
    return result;
}

std::optional<float> pitch_threshold(const std::vector<OcrLine>& lines) {
    std::vector<float> pitches;
    for (std::size_t index = 0; index + 1 < lines.size(); ++index) {
        const auto& first = lines[index].bounds;
        const auto& second = lines[index + 1].bounds;
        const float pitch = second.y - first.y;
        const float overlap = std::min(first.right(), second.right()) - std::max(first.x, second.x);
        if (pitch > 0.0F && overlap > 0.0F &&
            overlap / std::max(1.0F, std::min(first.width, second.width)) >= 0.35F) {
            pitches.push_back(pitch);
        }
    }
    if (pitches.size() < 4) {
        return std::nullopt;
    }
    std::sort(pitches.begin(), pitches.end());
    float best_variance = std::numeric_limits<float>::max();
    std::size_t best_cut = 0;
    for (std::size_t index = 1; index < pitches.size(); ++index) {
        const float score = static_cast<float>(index) * variance(pitches, 0, index) +
                            static_cast<float>(pitches.size() - index) *
                                variance(pitches, index, pitches.size());
        if (score < best_variance) {
            best_variance = score;
            best_cut = index;
        }
    }
    if (best_cut < 3 || best_cut >= pitches.size()) {
        return std::nullopt;
    }
    const float low_mean = std::accumulate(pitches.begin(), pitches.begin() + best_cut, 0.0F) /
                           static_cast<float>(best_cut);
    const float high_mean = std::accumulate(pitches.begin() + best_cut, pitches.end(), 0.0F) /
                            static_cast<float>(pitches.size() - best_cut);
    if (high_mean < low_mean * 1.25F) {
        return std::nullopt;
    }
    return (pitches[best_cut - 1] + pitches[best_cut]) / 2.0F;
}

std::vector<float> column_rights(const std::vector<OcrLine>& lines) {
    std::vector<float> result(lines.size());
    for (std::size_t index = 0; index < lines.size(); ++index) {
        const auto& current = lines[index].bounds;
        std::vector<float> peers{current.right()};
        for (std::size_t other_index = 0; other_index < lines.size(); ++other_index) {
            if (index == other_index) {
                continue;
            }
            const auto& other = lines[other_index].bounds;
            const float overlap = std::min(current.right(), other.right()) -
                                  std::max(current.x, other.x);
            if (overlap > 0.0F &&
                overlap / std::max(1.0F, std::min(current.width, other.width)) >= 0.5F) {
                peers.push_back(other.right());
            }
        }
        std::sort(peers.begin(), peers.end());
        const auto percentile = static_cast<std::size_t>(
            std::lround(static_cast<double>(peers.size() - 1) * 0.85));
        result[index] = peers[std::min(peers.size() - 1, percentile)];
    }
    return result;
}

bool clipped_by_edge(const TextBlock& block) {
    if (block.lines.size() < 4) {
        return false;
    }
    float edge = 0.0F;
    for (std::size_t index = 0; index + 1 < block.lines.size(); ++index) {
        edge = std::max(edge, block.lines[index].bounds.right());
    }
    int flush = 0;
    int short_lines = 0;
    const float width = block.bounds().width;
    for (std::size_t index = 0; index + 1 < block.lines.size(); ++index) {
        const float right = block.lines[index].bounds.right();
        flush += right >= edge - 2.0F ? 1 : 0;
        short_lines += right < edge - std::max(8.0F, width * 0.06F) ? 1 : 0;
    }
    return flush >= 3 && short_lines >= 1;
}

bool looks_like_paragraph(const TextBlock& block) {
    if (block.lines.size() < 3) {
        return true;
    }
    if (clipped_by_edge(block)) {
        return false;
    }
    const auto bounds = block.bounds();
    const float tolerance = std::max(4.0F, bounds.width * 0.12F);
    int full = 0;
    for (std::size_t index = 0; index + 1 < block.lines.size(); ++index) {
        full += block.lines[index].bounds.right() >= bounds.right() - tolerance ? 1 : 0;
    }
    return static_cast<float>(full) / static_cast<float>(block.lines.size() - 1) >= 0.7F;
}

std::vector<TextBlock> split_non_paragraphs(std::vector<TextBlock> blocks) {
    std::vector<TextBlock> output;
    for (auto& block : blocks) {
        if (looks_like_paragraph(block)) {
            output.push_back(std::move(block));
        } else {
            for (auto& line : block.lines) {
                TextBlock single;
                single.lines.push_back(std::move(line));
                output.push_back(std::move(single));
            }
        }
    }
    return output;
}

std::vector<TextBlock> split_leading_headings(std::vector<TextBlock> blocks) {
    std::vector<TextBlock> output;
    for (auto& block : blocks) {
        if (block.lines.size() < 2) {
            output.push_back(std::move(block));
            continue;
        }
        const auto& head = block.lines.front().bounds;
        std::vector<float> heights;
        float rest_width = 0.0F;
        for (std::size_t index = 1; index < block.lines.size(); ++index) {
            heights.push_back(block.lines[index].bounds.height);
            rest_width = std::max(rest_width, block.lines[index].bounds.width);
        }
        if (head.width > rest_width * 0.55F) {
            output.push_back(std::move(block));
            continue;
        }
        const float rest_height = median(std::move(heights));
        const bool taller = head.height > rest_height * 1.12F;
        bool looser = false;
        if (block.lines.size() >= 3) {
            std::vector<float> inner;
            for (std::size_t index = 1; index + 1 < block.lines.size(); ++index) {
                inner.push_back(block.lines[index + 1].bounds.y - block.lines[index].bounds.bottom());
            }
            looser = block.lines[1].bounds.y - head.bottom() > median(std::move(inner)) * 1.5F + 1.5F;
        }
        if (taller || looser) {
            TextBlock heading;
            heading.lines.push_back(std::move(block.lines.front()));
            TextBlock rest;
            rest.lines.insert(rest.lines.end(),
                              std::make_move_iterator(block.lines.begin() + 1),
                              std::make_move_iterator(block.lines.end()));
            output.push_back(std::move(heading));
            output.push_back(std::move(rest));
        } else {
            output.push_back(std::move(block));
        }
    }
    return output;
}

}  // namespace

RectF TextBlock::bounds() const {
    if (lines.empty()) {
        return {};
    }
    float left = lines.front().bounds.x;
    float top = lines.front().bounds.y;
    float right = lines.front().bounds.right();
    float bottom = lines.front().bounds.bottom();
    for (const auto& line : lines) {
        left = std::min(left, line.bounds.x);
        top = std::min(top, line.bounds.y);
        right = std::max(right, line.bounds.right());
        bottom = std::max(bottom, line.bounds.bottom());
    }
    return {left, top, right - left, bottom - top};
}

float TextBlock::line_height() const {
    std::vector<float> heights;
    heights.reserve(lines.size());
    for (const auto& line : lines) {
        heights.push_back(line.bounds.height);
    }
    return median(std::move(heights));
}

std::wstring TextBlock::recognized_text() const {
    std::wstring output;
    for (const auto& line : lines) {
        std::wstring current = line.text;
        current.erase(current.begin(),
                      std::find_if_not(current.begin(), current.end(), [](wchar_t value) {
                          return std::iswspace(value) != 0;
                      }));
        current.erase(std::find_if_not(current.rbegin(), current.rend(), [](wchar_t value) {
                          return std::iswspace(value) != 0;
                      }).base(), current.end());
        if (current.empty()) {
            continue;
        }
        if (!output.empty()) {
            if (is_cjk(output.back()) || is_cjk(current.front())) {
                // CJK line wrapping does not introduce a space.
            } else if (output.back() == L'-') {
                output.pop_back();
            } else {
                output.push_back(L' ');
            }
        }
        output += current;
    }
    return output;
}

bool TextBlock::centered() const {
    if (lines.size() < 2) {
        return false;
    }
    float min_left = lines.front().bounds.x;
    float max_left = min_left;
    float min_center = lines.front().bounds.center_x();
    float max_center = min_center;
    for (const auto& line : lines) {
        min_left = std::min(min_left, line.bounds.x);
        max_left = std::max(max_left, line.bounds.x);
        min_center = std::min(min_center, line.bounds.center_x());
        max_center = std::max(max_center, line.bounds.center_x());
    }
    const float left_spread = max_left - min_left;
    const float center_spread = max_center - min_center;
    return left_spread > bounds().width * 0.08F && center_spread * 2.0F < left_spread;
}

std::vector<TextBlock> group_lines(std::vector<OcrLine> lines, float pitch_ratio) {
    lines.erase(std::remove_if(lines.begin(), lines.end(), [](const OcrLine& line) {
        return std::all_of(line.text.begin(), line.text.end(), [](wchar_t value) {
            return std::iswspace(value) != 0;
        });
    }), lines.end());
    if (lines.empty()) {
        return {};
    }
    std::sort(lines.begin(), lines.end(), [](const OcrLine& left, const OcrLine& right) {
        const long left_row = std::lround(left.bounds.y / 4.0F);
        const long right_row = std::lround(right.bounds.y / 4.0F);
        return left_row == right_row ? left.bounds.x < right.bounds.x : left_row < right_row;
    });
    const auto rights = column_rights(lines);
    for (std::size_t index = 0; index < lines.size(); ++index) {
        lines[index].layout_column_right = rights[index];
    }
    const auto threshold = pitch_threshold(lines);

    std::vector<TextBlock> blocks;
    for (std::size_t line_index = 0; line_index < lines.size(); ++line_index) {
        auto& line = lines[line_index];
        bool placed = false;
        for (auto iterator = blocks.rbegin(); iterator != blocks.rend(); ++iterator) {
            auto& block = *iterator;
            const auto& last = block.lines.back();
            const float pitch = line.bounds.y - last.bounds.y;
            const float reference_height = std::max(last.bounds.height, line.bounds.height);
            if (pitch < -reference_height * 0.4F) {
                continue;
            }
            const auto observed = observed_pitch(block);
            float limit = observed ? *observed * 1.28F + 2.0F : reference_height * pitch_ratio;
            if (threshold) {
                limit = std::min(limit, *threshold);
            }
            if (pitch > limit) {
                continue;
            }
            const float expected_right = last.layout_column_right >= 0.0F
                ? last.layout_column_right
                : last.bounds.right();
            if (!observed && last.bounds.right() < expected_right - reference_height * 2.5F) {
                continue;
            }
            const float ratio = line.bounds.height / std::max(last.bounds.height, 0.001F);
            if (ratio < 0.45F || ratio > 2.0F) {
                continue;
            }
            const auto block_bounds = block.bounds();
            const float overlap = std::min(line.bounds.right(), block_bounds.right()) -
                                  std::max(line.bounds.x, block_bounds.x);
            if (overlap <= 0.0F ||
                overlap / std::max(1.0F, std::min(line.bounds.width, block_bounds.width)) < 0.35F) {
                continue;
            }
            block.lines.push_back(std::move(line));
            placed = true;
            break;
        }
        if (!placed) {
            TextBlock block;
            block.lines.push_back(std::move(line));
            blocks.push_back(std::move(block));
        }
    }
    blocks = split_leading_headings(std::move(blocks));
    blocks = split_non_paragraphs(std::move(blocks));
    std::sort(blocks.begin(), blocks.end(), [](const TextBlock& left, const TextBlock& right) {
        const auto a = left.bounds();
        const auto b = right.bounds();
        return a.y == b.y ? a.x < b.x : a.y < b.y;
    });
    return blocks;
}

}  // namespace screentrans
