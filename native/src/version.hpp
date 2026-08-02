#pragma once

#include <string_view>

#ifndef SCREENTRANS_VERSION_WIDE
#define SCREENTRANS_VERSION_WIDE L"0.0.0"
#endif

namespace screentrans {

inline constexpr std::wstring_view native_version = SCREENTRANS_VERSION_WIDE;

}  // namespace screentrans
