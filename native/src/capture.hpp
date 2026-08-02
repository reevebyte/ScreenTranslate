#pragma once

#include "types.hpp"

#include <windows.h>

namespace screentrans {

struct DesktopImage {
    RECT bounds{};
    PixelBuffer pixels;
};

PixelBuffer capture_rect(const RECT& physical_rect);
DesktopImage capture_virtual_desktop();

}  // namespace screentrans
