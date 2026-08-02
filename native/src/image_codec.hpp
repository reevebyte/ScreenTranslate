#pragma once

#include "types.hpp"

#include <cstdint>
#include <vector>

namespace screentrans {

std::vector<std::uint8_t> encode_png(const PixelBuffer& image);
PixelBuffer resize_bgra(const PixelBuffer& image, int width, int height);

}  // namespace screentrans
