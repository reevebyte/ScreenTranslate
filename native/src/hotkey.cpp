#include "hotkey.hpp"

#include "util.hpp"

#include <algorithm>
#include <cwctype>
#include <map>
#include <vector>

namespace screentrans {

std::optional<HotkeySpec> parse_hotkey(std::wstring_view value, std::wstring* error) {
    std::vector<std::wstring> parts;
    std::size_t start = 0;
    while (start <= value.size()) {
        const auto end = value.find(L'+', start);
        auto part = lower_ascii(trim(std::wstring(value.substr(
            start, end == std::wstring_view::npos ? value.size() - start : end - start))));
        if (!part.empty()) parts.push_back(std::move(part));
        if (end == std::wstring_view::npos) break;
        start = end + 1;
    }
    if (parts.empty()) {
        if (error) *error = L"快捷键不能为空";
        return std::nullopt;
    }
    HotkeySpec result;
    for (std::size_t index = 0; index + 1 < parts.size(); ++index) {
        const auto& part = parts[index];
        if (part == L"ctrl" || part == L"control") result.modifiers |= MOD_CONTROL;
        else if (part == L"alt") result.modifiers |= MOD_ALT;
        else if (part == L"shift") result.modifiers |= MOD_SHIFT;
        else if (part == L"win" || part == L"windows") result.modifiers |= MOD_WIN;
        else {
            if (error) *error = L"无法识别修饰键：" + part;
            return std::nullopt;
        }
    }
    const auto& key = parts.back();
    if (key.size() == 1 && ((key[0] >= L'a' && key[0] <= L'z') ||
                            (key[0] >= L'0' && key[0] <= L'9'))) {
        result.virtual_key = static_cast<UINT>(std::towupper(key[0]));
    } else if (key.size() >= 2 && key[0] == L'f') {
        try {
            const int function = std::stoi(key.substr(1));
            if (function >= 1 && function <= 24) result.virtual_key = VK_F1 + function - 1;
        } catch (...) {
        }
    } else {
        static const std::map<std::wstring, UINT, std::less<>> keys{
            {L"space", VK_SPACE}, {L"tab", VK_TAB}, {L"enter", VK_RETURN},
            {L"esc", VK_ESCAPE}, {L"escape", VK_ESCAPE}, {L"backspace", VK_BACK},
            {L"delete", VK_DELETE}, {L"insert", VK_INSERT}, {L"home", VK_HOME},
            {L"end", VK_END}, {L"pageup", VK_PRIOR}, {L"pagedown", VK_NEXT},
            {L"up", VK_UP}, {L"down", VK_DOWN}, {L"left", VK_LEFT}, {L"right", VK_RIGHT},
        };
        if (const auto found = keys.find(key); found != keys.end()) result.virtual_key = found->second;
    }
    if (!result.virtual_key) {
        if (error) *error = L"无法识别按键：" + key;
        return std::nullopt;
    }
    result.modifiers |= MOD_NOREPEAT;
    return result;
}

bool register_hotkey(HWND window, int identifier, std::wstring_view value,
                     std::wstring* error) {
    UnregisterHotKey(window, identifier);
    const auto parsed = parse_hotkey(value, error);
    if (!parsed) return false;
    if (!RegisterHotKey(window, identifier, parsed->modifiers, parsed->virtual_key)) {
        if (error) {
            *error = GetLastError() == ERROR_HOTKEY_ALREADY_REGISTERED
                ? L"快捷键已被其他程序占用"
                : L"Windows 拒绝注册这个快捷键";
        }
        return false;
    }
    return true;
}

}  // namespace screentrans
