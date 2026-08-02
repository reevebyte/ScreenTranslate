#pragma once

#include "util.hpp"

#include <winhttp.h>

#include <cstdint>
#include <functional>
#include <map>
#include <span>
#include <stop_token>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace screentrans {

inline constexpr std::size_t max_json_response = 4 * 1024 * 1024;
inline constexpr std::size_t max_text_response = 2 * 1024 * 1024;

class HttpError : public AppError {
public:
    HttpError(std::string message, DWORD status = 0)
        : AppError(std::move(message)), status_(status) {}
    [[nodiscard]] DWORD status() const noexcept { return status_; }

private:
    DWORD status_{};
};

struct HttpRequest {
    std::wstring method{L"GET"};
    std::wstring url;
    std::vector<std::pair<std::wstring, std::wstring>> headers;
    std::vector<std::uint8_t> body;
    int timeout_ms{20000};
    std::size_t max_response_bytes{max_json_response};
    // When set, response bytes are streamed to the callback instead of being
    // retained in HttpResponse::body. The status code is supplied so redirect
    // bodies can be ignored without buffering them.
    std::function<void(std::span<const std::uint8_t>, DWORD)> response_chunk;
};

struct HttpResponse {
    DWORD status{};
    std::wstring final_url;
    std::vector<std::uint8_t> body;
    std::map<std::wstring, std::wstring, std::less<>> headers;

    [[nodiscard]] std::string utf8() const;
    [[nodiscard]] std::wstring text() const;
    [[nodiscard]] std::wstring header(std::wstring_view name) const;
};

class HttpClient {
public:
    HttpClient();
    ~HttpClient();
    HttpClient(const HttpClient&) = delete;
    HttpClient& operator=(const HttpClient&) = delete;

    HttpResponse send(const HttpRequest& request, std::stop_token stop = {}) const;

private:
    HINTERNET session_{};
};

void require_success(const HttpResponse& response, std::wstring_view label,
                     std::span<const std::wstring> secrets = {});
std::wstring validate_api_base_url(std::wstring_view value,
                                   std::wstring_view label,
                                   bool key_present = true,
                                   bool allow_keyless_loopback_http = false);
std::wstring validate_same_origin_https(std::wstring_view value,
                                        std::wstring_view base_url,
                                        std::wstring_view label);
std::wstring append_query(
    std::wstring_view url,
    const std::vector<std::pair<std::wstring, std::wstring>>& parameters);
std::vector<std::uint8_t> utf8_body(std::wstring_view text);
std::vector<std::uint8_t> form_body(
    const std::vector<std::pair<std::wstring, std::wstring>>& fields);
std::wstring redact_sensitive(std::wstring value,
                              std::span<const std::wstring> secrets = {});

}  // namespace screentrans
