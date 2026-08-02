#include "language.hpp"

#include <algorithm>
#include <array>
#include <cwctype>

namespace screentrans {

namespace {

bool is_han(wchar_t value) noexcept {
    const auto code = static_cast<unsigned>(value);
    return (code >= 0x3400 && code <= 0x4DBF) ||
           (code >= 0x4E00 && code <= 0x9FFF) ||
           (code >= 0xF900 && code <= 0xFAFF);
}

bool is_ascii_letter(wchar_t value) noexcept {
    return (value >= L'A' && value <= L'Z') || (value >= L'a' && value <= L'z');
}

int latin_evidence(std::wstring_view text, int raw_count) {
    int evidence = raw_count;
    std::size_t index = 0;
    while (index < text.size()) {
        if (!is_ascii_letter(text[index])) {
            ++index;
            continue;
        }
        const std::size_t start = index++;
        int letters = 1;
        int uppercase = text[start] >= L'A' && text[start] <= L'Z' ? 1 : 0;
        int lowercase = text[start] >= L'a' && text[start] <= L'z' ? 1 : 0;
        bool code_marks = false;
        while (index < text.size()) {
            const wchar_t value = text[index];
            if (is_ascii_letter(value)) {
                ++letters;
                uppercase += value >= L'A' && value <= L'Z' ? 1 : 0;
                lowercase += value >= L'a' && value <= L'z' ? 1 : 0;
                ++index;
            } else if ((value >= L'0' && value <= L'9') ||
                       value == L'_' || value == L'.' || value == L'/' ||
                       value == L':' || value == L'\\' || value == L'-') {
                code_marks = true;
                ++index;
            } else {
                break;
            }
        }
        const bool camel = uppercase >= 2 && lowercase >= 1;
        const bool touches_han = (start > 0 && is_han(text[start - 1])) ||
                                 (index < text.size() && is_han(text[index]));
        if (camel || code_marks || touches_han) {
            evidence -= std::max(0, letters - 2);
        }
    }
    return std::max(0, evidence);
}

}  // namespace

bool is_cjk(wchar_t value) noexcept {
    const auto code = static_cast<unsigned>(value);
    return (code >= 0x3000 && code <= 0x30FF) ||
           (code >= 0x3400 && code <= 0x9FFF) ||
           (code >= 0xAC00 && code <= 0xD7AF) ||
           (code >= 0xF900 && code <= 0xFAFF) ||
           (code >= 0xFF00 && code <= 0xFFEF);
}

ScriptCounts script_counts(std::wstring_view text) {
    ScriptCounts counts;
    for (const wchar_t value : text) {
        const auto code = static_cast<unsigned>(value);
        if ((code >= 0x3040 && code <= 0x30FF) ||
            (code >= 0x31F0 && code <= 0x31FF)) {
            ++counts.kana;
        } else if ((code >= 0xAC00 && code <= 0xD7AF) ||
                   (code >= 0x1100 && code <= 0x11FF)) {
            ++counts.hangul;
        } else if (is_han(value)) {
            ++counts.han;
        } else if (code >= 0x0400 && code <= 0x04FF) {
            ++counts.cyrillic;
        } else if (is_ascii_letter(value) || (code >= 0x00C0 && code < 0x0250)) {
            ++counts.latin;
        } else if (!std::iswspace(value)) {
            ++counts.other;
        }
    }
    return counts;
}

std::wstring detect_language(std::wstring_view text) {
    const auto counts = script_counts(text);
    if (counts.kana >= 1 && counts.kana * 12 >= counts.han) {
        return L"ja";
    }
    if (counts.hangul >= 1) {
        return L"ko";
    }
    if (counts.cyrillic > counts.latin && counts.cyrillic >= 2) {
        return L"ru";
    }
    const int latin = latin_evidence(text, counts.latin);
    if (counts.han >= 2 && counts.han * 3 >= latin) {
        return L"zh";
    }
    return L"other";
}

std::wstring target_for(std::wstring_view text, std::wstring_view chinese_target) {
    if (detect_language(text) != L"zh") {
        return L"zh-Hans";
    }
    constexpr std::array<std::wstring_view, 8> allowed{
        L"en", L"ja", L"ko", L"fr", L"de", L"es", L"ru", L"zh-Hant",
    };
    if (std::find(allowed.begin(), allowed.end(), chinese_target) != allowed.end()) {
        return std::wstring(chinese_target);
    }
    return L"en";
}

std::wstring target_display_name(std::wstring_view code) {
    constexpr std::array<std::pair<std::wstring_view, std::wstring_view>, 9> names{{
        {L"zh-Hans", L"简体中文"}, {L"zh-Hant", L"繁体中文"},
        {L"en", L"英语"}, {L"ja", L"日语"}, {L"ko", L"韩语"},
        {L"fr", L"法语"}, {L"de", L"德语"}, {L"es", L"西班牙语"},
        {L"ru", L"俄语"},
    }};
    for (const auto& [key, value] : names) {
        if (key == code) {
            return std::wstring(value);
        }
    }
    return std::wstring(code);
}

std::wstring target_prompt_name(std::wstring_view code) {
    constexpr std::array<std::pair<std::wstring_view, std::wstring_view>, 9> names{{
        {L"zh-Hans", L"Simplified Chinese"}, {L"zh-Hant", L"Traditional Chinese"},
        {L"en", L"English"}, {L"ja", L"Japanese"}, {L"ko", L"Korean"},
        {L"fr", L"French"}, {L"de", L"German"}, {L"es", L"Spanish"},
        {L"ru", L"Russian"},
    }};
    for (const auto& [key, value] : names) {
        if (key == code) {
            return std::wstring(value);
        }
    }
    return std::wstring(code);
}

bool matches_target(std::wstring_view text, std::wstring_view target) {
    const auto counts = script_counts(text);
    const int cjk = counts.han + counts.kana + counts.hangul;
    if (target == L"en" || target == L"fr" || target == L"de" || target == L"es" ||
        target == L"it" || target == L"pt" || target == L"nl" || target == L"vi") {
        return !(cjk >= 1 && cjk * 2 >= counts.latin);
    }
    const int letters = cjk + counts.latin + counts.cyrillic;
    if (letters == 0) {
        return true;
    }
    if (target == L"zh-Hans" || target == L"zh-Hant") {
        return counts.han >= 1 || (letters == 1 && counts.latin == 1);
    }
    if (target == L"ja") {
        return counts.kana >= 1 || counts.han >= 1 || (letters == 1 && counts.latin == 1);
    }
    if (target == L"ko") {
        return counts.hangul >= 1 || (letters == 1 && counts.latin == 1);
    }
    if (target == L"ru") {
        return counts.cyrillic >= 1 || (letters == 1 && counts.latin == 1);
    }
    return true;
}

}  // namespace screentrans
