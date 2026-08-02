#include "http.hpp"
#include "version.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cwctype>
#include <limits>
#include <sstream>

namespace screentrans {

namespace {

class InternetHandle {
public:
    explicit InternetHandle(HINTERNET value = nullptr) noexcept : value_(value) {}
    ~InternetHandle() { if (value_) WinHttpCloseHandle(value_); }
    InternetHandle(const InternetHandle&) = delete;
    InternetHandle& operator=(const InternetHandle&) = delete;
    [[nodiscard]] HINTERNET get() const noexcept { return value_; }
    [[nodiscard]] explicit operator bool() const noexcept { return value_ != nullptr; }
    HINTERNET release() noexcept {
        const auto value = value_;
        value_ = nullptr;
        return value;
    }

private:
    HINTERNET value_{};
};

struct ParsedUrl {
    std::wstring scheme;
    std::wstring host;
    std::wstring path;
    INTERNET_PORT port{};
    bool secure{};
    bool has_userinfo{};
};

ParsedUrl parse_url(std::wstring_view source) {
    std::wstring url(source);
    URL_COMPONENTS parts{};
    parts.dwStructSize = sizeof(parts);
    parts.dwSchemeLength = static_cast<DWORD>(-1);
    parts.dwHostNameLength = static_cast<DWORD>(-1);
    parts.dwUserNameLength = static_cast<DWORD>(-1);
    parts.dwPasswordLength = static_cast<DWORD>(-1);
    parts.dwUrlPathLength = static_cast<DWORD>(-1);
    parts.dwExtraInfoLength = static_cast<DWORD>(-1);
    if (!WinHttpCrackUrl(url.c_str(), static_cast<DWORD>(url.size()), 0, &parts)) {
        throw HttpError("invalid URL: " + wide_to_utf8(source));
    }
    ParsedUrl result;
    if (parts.lpszScheme) {
        result.scheme.assign(parts.lpszScheme, parts.dwSchemeLength);
    }
    if (parts.lpszHostName) {
        result.host.assign(parts.lpszHostName, parts.dwHostNameLength);
    }
    if (parts.lpszUrlPath && parts.dwUrlPathLength) {
        result.path.assign(parts.lpszUrlPath, parts.dwUrlPathLength);
    } else {
        result.path = L"/";
    }
    if (parts.lpszExtraInfo && parts.dwExtraInfoLength) {
        result.path.append(parts.lpszExtraInfo, parts.dwExtraInfoLength);
    }
    result.port = parts.nPort;
    result.secure = parts.nScheme == INTERNET_SCHEME_HTTPS;
    result.has_userinfo = parts.dwUserNameLength != 0 || parts.dwPasswordLength != 0;
    return result;
}

std::wstring query_header(HINTERNET request, DWORD query, const wchar_t* name = nullptr) {
    DWORD size = 0;
    WinHttpQueryHeaders(request, query, name, nullptr, &size, WINHTTP_NO_HEADER_INDEX);
    if (GetLastError() != ERROR_INSUFFICIENT_BUFFER || size == 0) {
        return {};
    }
    std::wstring output(size / sizeof(wchar_t), L'\0');
    if (!WinHttpQueryHeaders(request, query, name, output.data(), &size,
                             WINHTTP_NO_HEADER_INDEX)) {
        return {};
    }
    output.resize(size / sizeof(wchar_t));
    while (!output.empty() && output.back() == L'\0') {
        output.pop_back();
    }
    return output;
}

std::wstring replace_all(std::wstring value, std::wstring_view needle,
                         std::wstring_view replacement) {
    if (needle.empty()) {
        return value;
    }
    std::size_t position = 0;
    while ((position = value.find(needle, position)) != std::wstring::npos) {
        value.replace(position, needle.size(), replacement);
        position += replacement.size();
    }
    return value;
}

bool is_loopback(std::wstring host) {
    host = lower_ascii(std::move(host));
    if (host == L"localhost" || host == L"127.0.0.1" || host == L"[::1]" || host == L"::1") {
        return true;
    }
    if (host.starts_with(L"127.")) {
        return true;
    }
    return false;
}

}  // namespace

HttpClient::HttpClient() {
    const auto user_agent = L"Mozilla/5.0 ScreenTranslate/" + std::wstring(native_version);
    session_ = WinHttpOpen(user_agent.c_str(),
                           WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                           WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
    if (!session_) {
        throw_last_error("WinHttpOpen failed");
    }
    const DWORD decompression = WINHTTP_DECOMPRESSION_FLAG_GZIP |
                                WINHTTP_DECOMPRESSION_FLAG_DEFLATE;
    WinHttpSetOption(session_, WINHTTP_OPTION_DECOMPRESSION,
                     const_cast<DWORD*>(&decompression), sizeof(decompression));
}

HttpClient::~HttpClient() {
    if (session_) {
        WinHttpCloseHandle(session_);
    }
}

HttpResponse HttpClient::send(const HttpRequest& request, std::stop_token stop) const {
    if (!session_ || request.url.empty()) {
        throw HttpError("HTTP request URL is empty");
    }
    const auto parsed = parse_url(request.url);
    if (parsed.host.empty() || (!parsed.secure && lower_ascii(parsed.scheme) != L"http")) {
        throw HttpError("HTTP request has an unsupported URL");
    }
    if (parsed.has_userinfo) {
        throw HttpError("HTTP URL cannot contain credentials");
    }
    InternetHandle connection(WinHttpConnect(session_, parsed.host.c_str(), parsed.port, 0));
    if (!connection) {
        throw_last_error("WinHttpConnect failed");
    }
    const DWORD flags = parsed.secure ? WINHTTP_FLAG_SECURE : 0;
    InternetHandle handle(WinHttpOpenRequest(
        connection.get(), request.method.c_str(), parsed.path.c_str(), nullptr,
        WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, flags));
    if (!handle) {
        throw_last_error("WinHttpOpenRequest failed");
    }
    WinHttpSetTimeouts(handle.get(), request.timeout_ms, request.timeout_ms,
                       request.timeout_ms, request.timeout_ms);
    DWORD redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
    if (!WinHttpSetOption(handle.get(), WINHTTP_OPTION_REDIRECT_POLICY,
                          &redirect_policy, sizeof(redirect_policy))) {
        throw_last_error("disable HTTP redirects");
    }

    std::wstring header_block;
    for (const auto& [name, value] : request.headers) {
        if (name.find_first_of(L"\r\n:") != std::wstring::npos ||
            value.find_first_of(L"\r\n") != std::wstring::npos) {
            throw HttpError("invalid HTTP header");
        }
        header_block += name;
        header_block += L": ";
        header_block += value;
        header_block += L"\r\n";
    }
    if (stop.stop_requested()) {
        throw HttpError("request cancelled");
    }
    std::atomic<HINTERNET> cancellable{handle.get()};
    struct CancellationOwner {
        InternetHandle& handle;
        std::atomic<HINTERNET>& cancellable;
        ~CancellationOwner() {
            if (!cancellable.exchange(nullptr, std::memory_order_acq_rel)) {
                (void)handle.release();
            }
        }
    } cancellation_owner{handle, cancellable};
    std::stop_callback cancellation(stop, [&cancellable] {
        if (const auto request = cancellable.exchange(nullptr, std::memory_order_acq_rel)) {
            WinHttpCloseHandle(request);
        }
    });
    const void* body = request.body.empty() ? WINHTTP_NO_REQUEST_DATA : request.body.data();
    if (!WinHttpSendRequest(
            handle.get(), header_block.empty() ? WINHTTP_NO_ADDITIONAL_HEADERS : header_block.c_str(),
            header_block.empty() ? 0 : static_cast<DWORD>(header_block.size()),
            const_cast<void*>(body), static_cast<DWORD>(request.body.size()),
            static_cast<DWORD>(request.body.size()), 0)) {
        if (stop.stop_requested()) throw HttpError("request cancelled");
        throw_last_error("send HTTP request");
    }
    if (!WinHttpReceiveResponse(handle.get(), nullptr)) {
        if (stop.stop_requested()) throw HttpError("request cancelled");
        throw_last_error("receive HTTP response");
    }

    HttpResponse response;
    DWORD status_size = sizeof(response.status);
    if (!WinHttpQueryHeaders(handle.get(), WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                             WINHTTP_HEADER_NAME_BY_INDEX, &response.status, &status_size,
                             WINHTTP_NO_HEADER_INDEX)) {
        if (stop.stop_requested()) throw HttpError("request cancelled");
        throw_last_error("read HTTP status");
    }
    response.final_url = query_header(handle.get(), WINHTTP_QUERY_LOCATION);
    for (const auto* name : {L"Content-Type", L"Content-Length", L"Operation-Location",
                             L"ETag", L"Last-Modified"}) {
        auto value = query_header(handle.get(), WINHTTP_QUERY_CUSTOM, name);
        if (!value.empty()) {
            response.headers.emplace(lower_ascii(name), std::move(value));
        }
    }
    if (response.status >= 300 && response.status < 400 && !response.final_url.empty()) {
        return response;
    }
    if (const auto length = response.header(L"content-length"); !length.empty()) {
        try {
            const auto declared = std::stoull(length);
            if (declared > request.max_response_bytes) {
                throw HttpError("HTTP response is too large", response.status);
            }
        } catch (const std::invalid_argument&) {
        } catch (const std::out_of_range&) {
            throw HttpError("HTTP response has an invalid Content-Length", response.status);
        }
    }

    std::size_t total_received = 0;
    while (true) {
        if (stop.stop_requested()) {
            throw HttpError("request cancelled", response.status);
        }
        DWORD available = 0;
        if (!WinHttpQueryDataAvailable(handle.get(), &available)) {
            if (stop.stop_requested()) throw HttpError("request cancelled", response.status);
            throw_last_error("read HTTP response");
        }
        if (available == 0) {
            break;
        }
        const std::size_t old_size = response.body.size();
        if (total_received + available > request.max_response_bytes) {
            throw HttpError("HTTP response is too large", response.status);
        }
        std::vector<std::uint8_t> streamed;
        auto* destination = static_cast<std::uint8_t*>(nullptr);
        if (request.response_chunk) {
            streamed.resize(available);
            destination = streamed.data();
        } else {
            response.body.resize(old_size + available);
            destination = response.body.data() + old_size;
        }
        DWORD read = 0;
        if (!WinHttpReadData(handle.get(), destination, available, &read)) {
            if (stop.stop_requested()) throw HttpError("request cancelled", response.status);
            throw_last_error("read HTTP response");
        }
        total_received += read;
        if (request.response_chunk) {
            streamed.resize(read);
            if (!streamed.empty()) request.response_chunk(streamed, response.status);
        } else {
            response.body.resize(old_size + read);
        }
    }
    return response;
}

std::string HttpResponse::utf8() const {
    if (body.empty()) return {};
    std::size_t offset = body.size() >= 3 && body[0] == 0xEF && body[1] == 0xBB && body[2] == 0xBF
        ? 3
        : 0;
    return std::string(reinterpret_cast<const char*>(body.data() + offset), body.size() - offset);
}

std::wstring HttpResponse::text() const {
    try {
        return utf8_to_wide(utf8());
    } catch (...) {
        if (body.empty()) {
            return {};
        }
        const int count = MultiByteToWideChar(CP_ACP, 0,
            reinterpret_cast<const char*>(body.data()), static_cast<int>(body.size()), nullptr, 0);
        std::wstring output(static_cast<std::size_t>(std::max(0, count)), L'\0');
        if (count > 0) {
            MultiByteToWideChar(CP_ACP, 0, reinterpret_cast<const char*>(body.data()),
                                static_cast<int>(body.size()), output.data(), count);
        }
        return output;
    }
}

std::wstring HttpResponse::header(std::wstring_view name) const {
    const auto found = headers.find(lower_ascii(std::wstring(name)));
    return found == headers.end() ? std::wstring{} : found->second;
}

std::wstring redact_sensitive(std::wstring value, std::span<const std::wstring> secrets) {
    for (const auto& secret : secrets) {
        if (secret.size() >= 4) {
            value = replace_all(std::move(value), secret, L"[REDACTED]");
            value = replace_all(std::move(value), url_encode(secret), L"[REDACTED]");
        }
    }
    for (const auto prefix : {L"sk-", L"sk-ant-", L"nvapi-"}) {
        std::size_t start = 0;
        while ((start = value.find(prefix, start)) != std::wstring::npos) {
            std::size_t end = start + std::wcslen(prefix);
            while (end < value.size()) {
                const wchar_t ch = value[end];
                if (!(std::iswalnum(ch) || ch == L'.' || ch == L'_' || ch == L'-')) {
                    break;
                }
                ++end;
            }
            if (end - start >= std::wcslen(prefix) + 6) {
                value.replace(start, end - start, L"[REDACTED]");
                start += 10;
            } else {
                ++start;
            }
        }
    }
    return value;
}

void require_success(const HttpResponse& response, std::wstring_view label,
                     std::span<const std::wstring> secrets) {
    if (response.status >= 200 && response.status < 300) {
        return;
    }
    std::wstring detail = response.text();
    std::replace(detail.begin(), detail.end(), L'\r', L' ');
    std::replace(detail.begin(), detail.end(), L'\n', L' ');
    detail = redact_sensitive(std::move(detail), secrets);
    if (detail.size() > 300) {
        detail.resize(300);
    }
    std::wstring message(label);
    message += response.status >= 300 && response.status < 400
        ? L" refused an HTTP redirect ("
        : L" request failed (HTTP ";
    message += std::to_wstring(response.status);
    message += L")";
    if (!trim(detail).empty()) {
        message += L": ";
        message += detail;
    }
    throw HttpError(wide_to_utf8(message), response.status);
}

std::wstring validate_api_base_url(std::wstring_view value,
                                   std::wstring_view label,
                                   bool key_present,
                                   bool allow_keyless_loopback_http) {
    std::wstring url = trim(std::wstring(value));
    if (url.empty() || url.find(L'\\') != std::wstring::npos ||
        std::any_of(url.begin(), url.end(), [](wchar_t ch) {
            return std::iswspace(ch) || ch < 32;
        })) {
        throw HttpError(wide_to_utf8(label) + " is not a safe URL");
    }
    const auto parsed = parse_url(url);
    if (parsed.host.empty() || parsed.has_userinfo || parsed.path.find(L'#') != std::wstring::npos ||
        parsed.path.find(L'?') != std::wstring::npos) {
        throw HttpError(wide_to_utf8(label) + " is not a safe base URL");
    }
    if (!parsed.secure && !(allow_keyless_loopback_http && !key_present &&
                            lower_ascii(parsed.scheme) == L"http" && is_loopback(parsed.host))) {
        throw HttpError(wide_to_utf8(label) + " must use HTTPS");
    }
    while (url.size() > 8 && url.back() == L'/') {
        url.pop_back();
    }
    return url;
}

std::wstring validate_same_origin_https(std::wstring_view value,
                                        std::wstring_view base_url,
                                        std::wstring_view label) {
    const auto candidate = parse_url(value);
    const auto base = parse_url(base_url);
    if (!candidate.secure || !base.secure || candidate.has_userinfo ||
        candidate.host.empty() || lower_ascii(candidate.host) != lower_ascii(base.host) ||
        candidate.port != base.port || candidate.path.find(L'#') != std::wstring::npos) {
        throw HttpError(wide_to_utf8(label) + " must use the configured HTTPS origin");
    }
    return std::wstring(value);
}

std::wstring append_query(
    std::wstring_view url,
    const std::vector<std::pair<std::wstring, std::wstring>>& parameters) {
    std::wstring output(url);
    wchar_t separator = output.find(L'?') == std::wstring::npos ? L'?' : L'&';
    for (const auto& [name, value] : parameters) {
        output.push_back(separator);
        separator = L'&';
        output += url_encode(name);
        output.push_back(L'=');
        output += url_encode(value);
    }
    return output;
}

std::vector<std::uint8_t> utf8_body(std::wstring_view text) {
    const auto encoded = wide_to_utf8(text);
    return {encoded.begin(), encoded.end()};
}

std::vector<std::uint8_t> form_body(
    const std::vector<std::pair<std::wstring, std::wstring>>& fields) {
    std::wstring value;
    for (const auto& [name, content] : fields) {
        if (!value.empty()) {
            value.push_back(L'&');
        }
        value += url_encode(name);
        value.push_back(L'=');
        value += url_encode(content);
    }
    return utf8_body(value);
}

}  // namespace screentrans
