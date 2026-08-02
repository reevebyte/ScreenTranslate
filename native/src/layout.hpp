#pragma once

#include "types.hpp"

#include <vector>

namespace screentrans {

std::vector<TextBlock> group_lines(std::vector<OcrLine> lines, float pitch_ratio = 1.9F);

}  // namespace screentrans
