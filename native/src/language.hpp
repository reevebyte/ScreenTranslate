#pragma once

#include <string>
#include <string_view>

namespace screentrans {

struct ScriptCounts {
    int han{};
    int kana{};
    int hangul{};
    int latin{};
    int cyrillic{};
    int other{};
};

ScriptCounts script_counts(std::wstring_view text);
std::wstring detect_language(std::wstring_view text);
std::wstring target_for(std::wstring_view text, std::wstring_view chinese_target = L"en");
std::wstring target_display_name(std::wstring_view code);
std::wstring target_prompt_name(std::wstring_view code);
bool matches_target(std::wstring_view text, std::wstring_view target);
bool is_cjk(wchar_t value) noexcept;

}  // namespace screentrans
