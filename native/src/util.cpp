#include "util.hpp"

#include <objbase.h>
#include <wincrypt.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cwctype>
#include <fstream>
#include <iterator>
#include <sstream>

namespace screentrans {

[[noreturn]] void throw_last_error(std::string_view operation, DWORD code) {
    LPWSTR raw = nullptr;
    const DWORD size = FormatMessageW(
        FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
            FORMAT_MESSAGE_IGNORE_INSERTS,
        nullptr, code, 0, reinterpret_cast<LPWSTR>(&raw), 0, nullptr);
    std::wstring detail = size && raw ? std::wstring(raw, size) : L"unknown Windows error";
    if (raw) {
        LocalFree(raw);
    }
    throw AppError(std::string(operation) + ": " + wide_to_utf8(trim(std::move(detail))));
}

std::wstring utf8_to_wide(std::string_view value) {
    if (value.empty()) {
        return {};
    }
    const int count = MultiByteToWideChar(
        CP_UTF8, MB_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()), nullptr, 0);
    if (count <= 0) {
        throw_last_error("UTF-8 decoding failed");
    }
    std::wstring out(static_cast<std::size_t>(count), L'\0');
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), out.data(), count) != count) {
        throw_last_error("UTF-8 decoding failed");
    }
    return out;
}

std::string wide_to_utf8(std::wstring_view value) {
    if (value.empty()) {
        return {};
    }
    const int count = WideCharToMultiByte(
        CP_UTF8, WC_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
        nullptr, 0, nullptr, nullptr);
    if (count <= 0) {
        throw_last_error("UTF-8 encoding failed");
    }
    std::string out(static_cast<std::size_t>(count), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), out.data(), count,
                            nullptr, nullptr) != count) {
        throw_last_error("UTF-8 encoding failed");
    }
    return out;
}

std::wstring trim(std::wstring value) {
    const auto blank = [](wchar_t ch) { return std::iswspace(ch) != 0; };
    value.erase(value.begin(), std::find_if_not(value.begin(), value.end(), blank));
    value.erase(std::find_if_not(value.rbegin(), value.rend(), blank).base(), value.end());
    return value;
}

std::wstring lower_ascii(std::wstring value) {
    for (auto& ch : value) {
        if (ch >= L'A' && ch <= L'Z') {
            ch = static_cast<wchar_t>(ch - L'A' + L'a');
        }
    }
    return value;
}

std::filesystem::path executable_path() {
    std::vector<wchar_t> buffer(512);
    for (;;) {
        SetLastError(ERROR_SUCCESS);
        const DWORD written = GetModuleFileNameW(
            nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
        if (written == 0) throw_last_error("read executable path");
        if (written < buffer.size() - 1 ||
            (written < buffer.size() && GetLastError() != ERROR_INSUFFICIENT_BUFFER)) {
            buffer.resize(written);
            return std::filesystem::path(buffer.begin(), buffer.end());
        }
        if (buffer.size() >= 32'768) {
            throw AppError("executable path is too long");
        }
        buffer.resize(std::min<std::size_t>(32'768, buffer.size() * 2));
    }
}

std::filesystem::path app_data_directory() {
    DWORD needed = GetEnvironmentVariableW(L"APPDATA", nullptr, 0);
    if (needed == 0) {
        throw_last_error("APPDATA is unavailable");
    }
    std::wstring value(needed, L'\0');
    const DWORD written = GetEnvironmentVariableW(L"APPDATA", value.data(), needed);
    if (written == 0 || written >= needed) {
        throw_last_error("APPDATA is unavailable");
    }
    value.resize(written);
    return std::filesystem::path(value) / L"ScreenTranslate";
}

COLORREF parse_rgb_color(std::wstring_view text, COLORREF fallback) noexcept {
    if (text.size() != 7 || text.front() != L'#') return fallback;
    unsigned value = 0;
    for (const wchar_t character : text.substr(1)) {
        value <<= 4;
        if (character >= L'0' && character <= L'9') value += character - L'0';
        else if (character >= L'a' && character <= L'f') value += character - L'a' + 10;
        else if (character >= L'A' && character <= L'F') value += character - L'A' + 10;
        else return fallback;
    }
    return RGB((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF);
}

COLORREF adjust_rgb(COLORREF color, int amount) noexcept {
    const auto channel = [amount](BYTE value) {
        return static_cast<BYTE>(std::clamp(static_cast<int>(value) + amount, 0, 255));
    };
    return RGB(channel(GetRValue(color)), channel(GetGValue(color)),
               channel(GetBValue(color)));
}

std::string read_utf8_file(const std::filesystem::path& path) {
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        throw AppError("cannot open " + wide_to_utf8(path.wstring()));
    }
    return {std::istreambuf_iterator<char>(stream), std::istreambuf_iterator<char>()};
}

void write_utf8_file_atomic(const std::filesystem::path& path, std::string_view content) {
    std::filesystem::create_directories(path.parent_path());
    auto temporary = path;
    temporary += L".tmp";
    {
        std::ofstream stream(temporary, std::ios::binary | std::ios::trunc);
        if (!stream) {
            throw AppError("cannot write " + wide_to_utf8(temporary.wstring()));
        }
        stream.write(content.data(), static_cast<std::streamsize>(content.size()));
        stream.flush();
        if (!stream) {
            throw AppError("cannot write " + wide_to_utf8(temporary.wstring()));
        }
    }
    if (!MoveFileExW(temporary.c_str(), path.c_str(),
                     MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
        const DWORD replace_error = GetLastError();
        if (!CopyFileW(temporary.c_str(), path.c_str(), FALSE)) {
            DeleteFileW(temporary.c_str());
            throw_last_error("cannot replace configuration", replace_error);
        }
        DeleteFileW(temporary.c_str());
    }
}

std::string base64_encode(std::span<const std::uint8_t> bytes) {
    if (bytes.empty()) {
        return {};
    }
    DWORD count = 0;
    if (!CryptBinaryToStringA(bytes.data(), static_cast<DWORD>(bytes.size()),
                              CRYPT_STRING_BASE64 | CRYPT_STRING_NOCRLF, nullptr, &count)) {
        throw_last_error("base64 encoding failed");
    }
    std::string out(count, '\0');
    if (!CryptBinaryToStringA(bytes.data(), static_cast<DWORD>(bytes.size()),
                              CRYPT_STRING_BASE64 | CRYPT_STRING_NOCRLF, out.data(), &count)) {
        throw_last_error("base64 encoding failed");
    }
    if (!out.empty() && out.back() == '\0') {
        out.pop_back();
    }
    return out;
}

std::vector<std::uint8_t> base64_decode(std::string_view text) {
    if (text.empty()) {
        return {};
    }
    DWORD count = 0;
    if (!CryptStringToBinaryA(text.data(), static_cast<DWORD>(text.size()),
                              CRYPT_STRING_BASE64, nullptr, &count, nullptr, nullptr)) {
        throw AppError("invalid base64 data");
    }
    std::vector<std::uint8_t> out(count);
    if (!CryptStringToBinaryA(text.data(), static_cast<DWORD>(text.size()),
                              CRYPT_STRING_BASE64, out.data(), &count, nullptr, nullptr)) {
        throw AppError("invalid base64 data");
    }
    out.resize(count);
    return out;
}

std::wstring url_encode_utf8(std::string_view value) {
    constexpr wchar_t hex[] = L"0123456789ABCDEF";
    std::wstring out;
    out.reserve(value.size() * 3);
    for (const unsigned char ch : value) {
        if ((ch >= 'A' && ch <= 'Z') || (ch >= 'a' && ch <= 'z') ||
            (ch >= '0' && ch <= '9') || ch == '-' || ch == '_' || ch == '.' || ch == '~') {
            out.push_back(static_cast<wchar_t>(ch));
        } else {
            out.push_back(L'%');
            out.push_back(hex[ch >> 4]);
            out.push_back(hex[ch & 0x0F]);
        }
    }
    return out;
}

std::wstring url_encode(std::wstring_view value) {
    return url_encode_utf8(wide_to_utf8(value));
}

std::wstring html_unescape(std::wstring value) {
    const std::array<std::pair<std::wstring_view, std::wstring_view>, 5> named{{
        {L"&amp;", L"&"}, {L"&lt;", L"<"}, {L"&gt;", L">"},
        {L"&quot;", L"\""}, {L"&#39;", L"'"},
    }};
    for (const auto& [encoded, decoded] : named) {
        std::size_t at = 0;
        while ((at = value.find(encoded, at)) != std::wstring::npos) {
            value.replace(at, encoded.size(), decoded);
            at += decoded.size();
        }
    }
    return value;
}

std::wstring new_uuid() {
    GUID value{};
    if (FAILED(CoCreateGuid(&value))) {
        throw AppError("cannot create UUID");
    }
    wchar_t buffer[40]{};
    const int count = StringFromGUID2(value, buffer, static_cast<int>(std::size(buffer)));
    if (count <= 2) {
        throw AppError("cannot format UUID");
    }
    return std::wstring(buffer + 1, buffer + count - 2);
}

bool constant_time_equal(std::span<const std::uint8_t> left,
                         std::span<const std::uint8_t> right) noexcept {
    if (left.size() != right.size()) {
        return false;
    }
    std::uint8_t difference = 0;
    for (std::size_t index = 0; index < left.size(); ++index) {
        difference |= left[index] ^ right[index];
    }
    return difference == 0;
}

void util_self_test() {
    constexpr COLORREF fallback = RGB(9, 8, 7);
    if (parse_rgb_color(L"#123456", fallback) != RGB(0x12, 0x34, 0x56) ||
        parse_rgb_color(L"#12xx56", fallback) != fallback ||
        adjust_rgb(RGB(250, 4, 100), 10) != RGB(255, 14, 110) ||
        adjust_rgb(RGB(5, 30, 0), -10) != RGB(0, 20, 0)) {
        throw AppError("Win32 utility color self-test failed");
    }
    std::error_code error;
    const auto path = executable_path();
    if (path.empty() || !std::filesystem::is_regular_file(path, error) || error) {
        throw AppError("Win32 utility executable path self-test failed");
    }
}

}  // namespace screentrans
