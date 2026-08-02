#pragma once

#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <winrt/base.h>

#ifdef GetObject
#undef GetObject
#endif

#include <filesystem>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace screentrans {

class AppError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

class WinrtApartment {
public:
    explicit WinrtApartment(winrt::apartment_type type) {
        winrt::init_apartment(type);
    }
    ~WinrtApartment() { winrt::uninit_apartment(); }

    WinrtApartment(const WinrtApartment&) = delete;
    WinrtApartment& operator=(const WinrtApartment&) = delete;
};

class UniqueHandle {
public:
    UniqueHandle() = default;
    explicit UniqueHandle(HANDLE value) noexcept : value_(value) {}
    ~UniqueHandle() { reset(); }

    UniqueHandle(const UniqueHandle&) = delete;
    UniqueHandle& operator=(const UniqueHandle&) = delete;
    UniqueHandle(UniqueHandle&& other) noexcept : value_(other.release()) {}
    UniqueHandle& operator=(UniqueHandle&& other) noexcept {
        if (this != &other) {
            reset(other.release());
        }
        return *this;
    }

    [[nodiscard]] HANDLE get() const noexcept { return value_; }
    [[nodiscard]] explicit operator bool() const noexcept {
        return value_ != nullptr && value_ != INVALID_HANDLE_VALUE;
    }
    HANDLE release() noexcept {
        const auto value = value_;
        value_ = nullptr;
        return value;
    }
    void reset(HANDLE value = nullptr) noexcept {
        if (*this) {
            CloseHandle(value_);
        }
        value_ = value;
    }

private:
    HANDLE value_{};
};

[[noreturn]] void throw_last_error(std::string_view operation, DWORD code = GetLastError());
std::wstring utf8_to_wide(std::string_view value);
std::string wide_to_utf8(std::wstring_view value);
std::wstring trim(std::wstring value);
std::wstring lower_ascii(std::wstring value);
std::filesystem::path executable_path();
std::filesystem::path app_data_directory();
COLORREF parse_rgb_color(std::wstring_view text,
                         COLORREF fallback = RGB(40, 199, 111)) noexcept;
COLORREF adjust_rgb(COLORREF color, int amount) noexcept;
std::string read_utf8_file(const std::filesystem::path& path);
void write_utf8_file_atomic(const std::filesystem::path& path, std::string_view content);
std::string base64_encode(std::span<const std::uint8_t> bytes);
std::vector<std::uint8_t> base64_decode(std::string_view text);
std::wstring url_encode(std::wstring_view value);
std::wstring url_encode_utf8(std::string_view value);
std::wstring html_unescape(std::wstring value);
std::wstring new_uuid();
bool constant_time_equal(std::span<const std::uint8_t> left,
                         std::span<const std::uint8_t> right) noexcept;
void util_self_test();

}  // namespace screentrans
