#pragma once

#include <windows.h>

#include <optional>
#include <string>
#include <string_view>

namespace screentrans {

struct HotkeySpec {
    UINT modifiers{};
    UINT virtual_key{};
};

std::optional<HotkeySpec> parse_hotkey(std::wstring_view value,
                                       std::wstring* error = nullptr);
bool register_hotkey(HWND window, int identifier, std::wstring_view value,
                     std::wstring* error = nullptr);

}  // namespace screentrans
