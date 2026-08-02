#include "updater.hpp"

#include "crypto.hpp"
#include "util.hpp"

#include <bcrypt.h>
#include <shellapi.h>
#include <winrt/Windows.Data.Json.h>
#include <winrt/Windows.Foundation.Collections.h>

#include <algorithm>
#include <array>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cwctype>
#include <fstream>
#include <map>
#include <set>
#include <span>
#include <sstream>
#include <vector>

namespace screentrans {

using namespace winrt::Windows::Data::Json;

namespace {

constexpr std::size_t manifest_limit = 256 * 1024;
constexpr int max_redirects = 8;
constexpr wchar_t manifest_name[] = L"update-manifest.json";
constexpr wchar_t helper_flag[] = L"--apply-update";

struct UrlParts {
    std::wstring scheme;
    std::wstring host;
    INTERNET_PORT port{};
    std::wstring path;
    std::wstring query;
    std::wstring fragment;
    bool userinfo{};
};

UrlParts parse_url(std::wstring_view input, std::wstring_view label) {
    if (input.empty() || input.find(L'\\') != std::wstring_view::npos ||
        std::any_of(input.begin(), input.end(), [](wchar_t ch) { return ch < 0x20 || ch == 0x7F; })) {
        throw UpdateError(wide_to_utf8(std::wstring(label) + L"无效"));
    }
    std::wstring value(input);
    URL_COMPONENTS parts{};
    parts.dwStructSize = sizeof(parts);
    parts.dwSchemeLength = static_cast<DWORD>(-1);
    parts.dwHostNameLength = static_cast<DWORD>(-1);
    parts.dwUserNameLength = static_cast<DWORD>(-1);
    parts.dwPasswordLength = static_cast<DWORD>(-1);
    parts.dwUrlPathLength = static_cast<DWORD>(-1);
    parts.dwExtraInfoLength = static_cast<DWORD>(-1);
    if (!WinHttpCrackUrl(value.c_str(), static_cast<DWORD>(value.size()), 0, &parts)) {
        throw UpdateError(wide_to_utf8(std::wstring(label) + L"格式无效"));
    }
    UrlParts result;
    if (parts.lpszScheme) result.scheme.assign(parts.lpszScheme, parts.dwSchemeLength);
    if (parts.lpszHostName) result.host.assign(parts.lpszHostName, parts.dwHostNameLength);
    result.port = parts.nPort;
    if (parts.lpszUrlPath) result.path.assign(parts.lpszUrlPath, parts.dwUrlPathLength);
    result.userinfo = parts.dwUserNameLength || parts.dwPasswordLength;
    if (parts.lpszExtraInfo && parts.dwExtraInfoLength) {
        std::wstring extra(parts.lpszExtraInfo, parts.dwExtraInfoLength);
        const auto hash = extra.find(L'#');
        result.query = extra.substr(0, hash);
        if (hash != std::wstring::npos) result.fragment = extra.substr(hash);
    }
    result.scheme = lower_ascii(std::move(result.scheme));
    result.host = lower_ascii(std::move(result.host));
    return result;
}

void require_https(const UrlParts& url, std::wstring_view label) {
    if (url.scheme != L"https" || url.host.empty() ||
        (url.port != INTERNET_DEFAULT_HTTPS_PORT && url.port != 0) ||
        url.userinfo || !url.fragment.empty()) {
        throw UpdateError(wide_to_utf8(std::wstring(label) + L"必须是标准 HTTPS 地址"));
    }
}

bool github_asset_host(std::wstring_view host) {
    return host == L"github.com" || host == L"objects.githubusercontent.com" ||
           (host.size() > 22 && host.ends_with(L".githubusercontent.com"));
}

void require_github_response_url(std::wstring_view url, std::wstring_view label) {
    const auto parsed = parse_url(url, label);
    require_https(parsed, label);
    if (!github_asset_host(parsed.host)) {
        throw UpdateError(wide_to_utf8(std::wstring(label) + L"被重定向到非 GitHub 地址"));
    }
}

std::vector<std::wstring> path_parts(std::wstring_view path) {
    std::vector<std::wstring> output;
    std::size_t start = 0;
    while (start < path.size()) {
        while (start < path.size() && path[start] == L'/') ++start;
        if (start >= path.size()) break;
        const auto end = path.find(L'/', start);
        output.emplace_back(path.substr(start, end == std::wstring_view::npos
            ? path.size() - start : end - start));
        if (end == std::wstring_view::npos) break;
        start = end + 1;
    }
    return output;
}

bool github_name(std::wstring_view value) {
    if (value.empty() || value == L"." || value == L"..") return false;
    return std::all_of(value.begin(), value.end(), [](wchar_t ch) {
        return (ch >= L'a' && ch <= L'z') || (ch >= L'A' && ch <= L'Z') ||
               (ch >= L'0' && ch <= L'9') || ch == L'_' || ch == L'.' || ch == L'-';
    });
}

struct Repository {
    std::wstring owner;
    std::wstring name;
    [[nodiscard]] std::wstring url() const { return L"https://github.com/" + owner + L"/" + name; }
};

Repository parse_repository(std::wstring_view input) {
    const auto url = parse_url(input, L"GitHub 仓库地址");
    require_https(url, L"GitHub 仓库地址");
    const auto parts = path_parts(url.path);
    if (url.host != L"github.com" || !url.query.empty() || parts.size() != 2 ||
        !github_name(parts[0]) || !github_name(parts[1])) {
        throw UpdateError("GitHub repository URL is invalid");
    }
    return {parts[0], parts[1]};
}

bool same_repository(const Repository& left, const Repository& right) {
    return lower_ascii(left.owner) == lower_ascii(right.owner) &&
           lower_ascii(left.name) == lower_ascii(right.name);
}

Repository parse_manifest_repository(std::wstring_view input) {
    const auto url = parse_url(input, L"更新清单地址");
    require_https(url, L"更新清单地址");
    const auto parts = path_parts(url.path);
    const bool latest = parts.size() == 6 && parts[2] == L"releases" &&
        parts[3] == L"latest" && parts[4] == L"download" && parts[5] == manifest_name;
    const bool tagged = parts.size() == 6 && parts[2] == L"releases" &&
        parts[3] == L"download" && !parts[4].empty() && parts[5] == manifest_name;
    if (url.host != L"github.com" || !url.query.empty() || !(latest || tagged) ||
        !github_name(parts[0]) || !github_name(parts[1])) {
        throw UpdateError("update manifest must be a GitHub Release asset");
    }
    return {parts[0], parts[1]};
}

struct SemverIdentifier {
    bool numeric{};
    std::uint64_t number{};
    std::wstring text;
};

struct Semver {
    std::uint64_t major{}, minor{}, patch{};
    std::vector<SemverIdentifier> prerelease;
};

std::uint64_t parse_number(std::wstring_view value, std::wstring_view label) {
    if (value.empty() || (value.size() > 1 && value.front() == L'0') ||
        !std::all_of(value.begin(), value.end(), [](wchar_t ch) { return ch >= L'0' && ch <= L'9'; })) {
        throw UpdateError(wide_to_utf8(std::wstring(label) + L"不符合 SemVer"));
    }
    try { return std::stoull(std::wstring(value)); }
    catch (...) { throw UpdateError(wide_to_utf8(std::wstring(label) + L"超出范围")); }
}

Semver parse_semver(std::wstring_view input) {
    if (input.empty() || input.size() > 128) throw UpdateError("version is not valid SemVer");
    const auto plus = input.find(L'+');
    if (plus != std::wstring_view::npos) {
        if (input.find(L'+', plus + 1) != std::wstring_view::npos || plus + 1 == input.size()) {
            throw UpdateError("version is not valid SemVer");
        }
        std::size_t offset = plus + 1;
        while (offset <= input.size()) {
            const auto end = input.find(L'.', offset);
            const auto item = input.substr(offset, end == std::wstring_view::npos
                ? input.size() - offset : end - offset);
            if (item.empty() || !std::all_of(item.begin(), item.end(), [](wchar_t ch) {
                    return (ch >= L'a' && ch <= L'z') || (ch >= L'A' && ch <= L'Z') ||
                           (ch >= L'0' && ch <= L'9') || ch == L'-';
                })) throw UpdateError("version is not valid SemVer");
            if (end == std::wstring_view::npos) break;
            offset = end + 1;
        }
    }
    auto value = input.substr(0, plus);
    const auto dash = value.find(L'-');
    const auto core = value.substr(0, dash);
    std::array<std::wstring_view, 3> numbers;
    std::size_t start = 0;
    for (int index = 0; index < 3; ++index) {
        const auto end = core.find(L'.', start);
        if ((index < 2 && end == std::wstring_view::npos) ||
            (index == 2 && end != std::wstring_view::npos)) {
            throw UpdateError("version is not valid SemVer");
        }
        numbers[index] = core.substr(start, end == std::wstring_view::npos
            ? core.size() - start : end - start);
        start = end == std::wstring_view::npos ? core.size() : end + 1;
    }
    Semver result{parse_number(numbers[0], L"版本号"),
                  parse_number(numbers[1], L"版本号"),
                  parse_number(numbers[2], L"版本号"), {}};
    if (dash != std::wstring_view::npos) {
        auto pre = value.substr(dash + 1);
        if (pre.empty()) throw UpdateError("version is not valid SemVer");
        std::size_t offset = 0;
        while (offset <= pre.size()) {
            const auto end = pre.find(L'.', offset);
            auto item = pre.substr(offset, end == std::wstring_view::npos
                ? pre.size() - offset : end - offset);
            if (item.empty() || !std::all_of(item.begin(), item.end(), [](wchar_t ch) {
                    return (ch >= L'a' && ch <= L'z') || (ch >= L'A' && ch <= L'Z') ||
                           (ch >= L'0' && ch <= L'9') || ch == L'-';
                })) throw UpdateError("version is not valid SemVer");
            const bool numeric = std::all_of(item.begin(), item.end(), [](wchar_t ch) {
                return ch >= L'0' && ch <= L'9';
            });
            result.prerelease.push_back({numeric, numeric ? parse_number(item, L"预发布版本") : 0,
                                         std::wstring(item)});
            if (end == std::wstring_view::npos) break;
            offset = end + 1;
        }
    }
    return result;
}

int compare_semver(const Semver& left, const Semver& right) {
    for (const auto [a, b] : {std::pair{left.major, right.major},
                              std::pair{left.minor, right.minor},
                              std::pair{left.patch, right.patch}}) {
        if (a != b) return a < b ? -1 : 1;
    }
    if (left.prerelease.empty() != right.prerelease.empty()) {
        return left.prerelease.empty() ? 1 : -1;
    }
    for (std::size_t index = 0; index < std::min(left.prerelease.size(), right.prerelease.size()); ++index) {
        const auto& a = left.prerelease[index];
        const auto& b = right.prerelease[index];
        if (a.numeric != b.numeric) return a.numeric ? -1 : 1;
        if (a.numeric && a.number != b.number) return a.number < b.number ? -1 : 1;
        if (!a.numeric && a.text != b.text) return a.text < b.text ? -1 : 1;
    }
    if (left.prerelease.size() == right.prerelease.size()) return 0;
    return left.prerelease.size() < right.prerelease.size() ? -1 : 1;
}

bool redirect_status(DWORD status) {
    return status == 301 || status == 302 || status == 303 || status == 307 || status == 308;
}

HttpResponse github_get(HttpClient& client, std::wstring url, std::size_t limit,
                        std::stop_token stop,
                        const std::function<void(std::span<const std::uint8_t>, DWORD)>& chunks = {}) {
    std::set<std::wstring> visited;
    for (int redirect = 0; redirect <= max_redirects; ++redirect) {
        require_github_response_url(url, L"更新响应地址");
        if (!visited.insert(lower_ascii(url)).second) throw UpdateError("update redirect loop detected");
        HttpRequest request;
        request.url = url;
        request.timeout_ms = 30'000;
        request.max_response_bytes = limit;
        request.headers = {
            {L"Accept", chunks ? L"application/octet-stream" : L"application/json"},
            {L"User-Agent", L"ScreenTranslate/" + std::wstring(native_version)},
        };
        request.response_chunk = chunks;
        auto response = client.send(request, stop);
        if (!redirect_status(response.status)) {
            require_success(response, L"GitHub update");
            return response;
        }
        if (response.final_url.empty()) throw UpdateError("GitHub redirect has no Location");
        url = response.final_url;
    }
    throw UpdateError("too many GitHub redirects");
}

std::wstring required_string(const JsonObject& object, std::wstring_view key) {
    if (!object.HasKey(key) || object.Lookup(key).ValueType() != JsonValueType::String) {
        throw UpdateError("update manifest is missing a string field");
    }
    return object.GetNamedString(key).c_str();
}

std::uint64_t required_integer(const JsonObject& object, std::wstring_view key) {
    if (!object.HasKey(key) || object.Lookup(key).ValueType() != JsonValueType::Number) {
        throw UpdateError("update manifest is missing a numeric field");
    }
    const double value = object.GetNamedNumber(key);
    if (value < 0 || value > static_cast<double>(maximum_update_size) || value != std::floor(value)) {
        throw UpdateError("update manifest contains an invalid size");
    }
    return static_cast<std::uint64_t>(value);
}

bool valid_sha256(std::wstring_view value) {
    return value.size() == 64 && std::all_of(value.begin(), value.end(), [](wchar_t ch) {
        return (ch >= L'0' && ch <= L'9') || (ch >= L'a' && ch <= L'f') ||
               (ch >= L'A' && ch <= L'F');
    });
}

UpdateInfo parse_manifest(std::string_view payload, const Repository& repository) {
    JsonObject root;
    try { root = JsonObject::Parse(utf8_to_wide(payload)); }
    catch (...) { throw UpdateError("update manifest is not valid UTF-8 JSON"); }
    if (root.GetNamedNumber(L"schema_version", 0) != 2 ||
        root.GetNamedString(L"product", L"") != L"ScreenTranslate") {
        throw UpdateError("unsupported update manifest");
    }
    UpdateInfo info;
    info.version = required_string(root, L"version");
    const auto semver = parse_semver(info.version);
    info.channel = required_string(root, L"channel");
    if (info.channel != L"stable" && info.channel != L"preview") {
        throw UpdateError("update channel is invalid");
    }
    if ((info.channel == L"stable") == !semver.prerelease.empty()) {
        throw UpdateError("update channel does not match version");
    }
    info.release_url = required_string(root, L"release_url");
    const auto expected_release = repository.url() + L"/releases/tag/v" + info.version;
    if (info.release_url != expected_release) throw UpdateError("release URL is not repository-pinned");
    info.published_at = root.GetNamedString(L"published_at", L"").c_str();
    const auto timestamp_t = info.published_at.find(L'T');
    const bool timezone = info.published_at.ends_with(L"Z") ||
        (timestamp_t != std::wstring::npos &&
         info.published_at.find_last_of(L"+-") > timestamp_t);
    if (info.published_at.empty() || info.published_at.size() > 64 ||
        timestamp_t == std::wstring::npos || !timezone ||
        !std::all_of(info.published_at.begin(), info.published_at.end(), [](wchar_t ch) {
            return (ch >= L'0' && ch <= L'9') || ch == L'T' || ch == L'Z' ||
                   ch == L'+' || ch == L'-' || ch == L':' || ch == L'.';
        })) throw UpdateError("published timestamp is invalid");
    const auto platform = root.GetNamedObject(L"platform", nullptr);
    if (!platform || platform.GetNamedString(L"os", L"") != L"windows" ||
        (platform.GetNamedString(L"arch", L"") != L"x86_64" &&
         platform.GetNamedString(L"arch", L"") != L"amd64")) {
        throw UpdateError("update is not for Windows x64");
    }
    const auto artifact = root.GetNamedObject(L"artifact", nullptr);
    if (!artifact) throw UpdateError("update manifest has no artifact");
    info.artifact.name = required_string(artifact, L"name");
    const auto expected_name = L"ScreenTranslate-" + info.version + L"-setup-x64.exe";
    if (info.artifact.name != expected_name) throw UpdateError("artifact name does not match version");
    info.artifact.url = required_string(artifact, L"url");
    const auto artifact_url = parse_url(info.artifact.url, L"安装包地址");
    require_https(artifact_url, L"安装包地址");
    const auto parts = path_parts(artifact_url.path);
    if (artifact_url.host != L"github.com" || !artifact_url.query.empty() || parts.size() != 6 ||
        lower_ascii(parts[0]) != lower_ascii(repository.owner) ||
        lower_ascii(parts[1]) != lower_ascii(repository.name) ||
        parts[2] != L"releases" || parts[3] != L"download" ||
        parts[4] != L"v" + info.version || parts[5] != info.artifact.name) {
        throw UpdateError("artifact URL is not repository and version pinned");
    }
    info.artifact.size = required_integer(artifact, L"size");
    if (info.artifact.size == 0) throw UpdateError("artifact size is invalid");
    info.artifact.sha256 = lower_ascii(required_string(artifact, L"sha256"));
    if (!valid_sha256(info.artifact.sha256)) throw UpdateError("artifact SHA-256 is invalid");
    return info;
}

std::filesystem::path update_root() {
    DWORD needed = GetEnvironmentVariableW(L"LOCALAPPDATA", nullptr, 0);
    if (!needed) throw_last_error("LOCALAPPDATA is unavailable");
    std::wstring value(needed, L'\0');
    const DWORD written = GetEnvironmentVariableW(L"LOCALAPPDATA", value.data(), needed);
    if (!written || written >= needed) throw_last_error("LOCALAPPDATA is unavailable");
    value.resize(written);
    return std::filesystem::path(value) / L"ScreenTranslate" / L"updates";
}

bool reparse_point(const std::filesystem::path& path) {
    const DWORD attributes = GetFileAttributesW(path.c_str());
    if (attributes == INVALID_FILE_ATTRIBUTES) {
        if (GetLastError() == ERROR_FILE_NOT_FOUND || GetLastError() == ERROR_PATH_NOT_FOUND) return false;
        throw_last_error("inspect update cache");
    }
    return (attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
}

void require_plain_path(const std::filesystem::path& path, std::wstring_view label) {
    if (reparse_point(path)) throw UpdateError(wide_to_utf8(std::wstring(label) + L"不能是重解析点"));
}

std::filesystem::path target_path(const UpdateInfo& info, bool create) {
    auto root = update_root();
    require_plain_path(root, L"更新缓存目录");
    if (create) {
        std::error_code error;
        std::filesystem::create_directories(root, error);
        if (error) throw UpdateError("cannot create update cache");
        require_plain_path(root, L"更新缓存目录");
    }
    auto version = root / (L"v" + info.version);
    require_plain_path(version, L"版本缓存目录");
    if (create) {
        std::error_code error;
        std::filesystem::create_directory(version, error);
        if (error && !std::filesystem::is_directory(version)) {
            throw UpdateError("cannot create version update cache");
        }
        require_plain_path(version, L"版本缓存目录");
    }
    auto target = version / info.artifact.name;
    if (target.parent_path() != version || version.parent_path() != root) {
        throw UpdateError("update cache path escaped its root");
    }
    return target;
}

void prune_cache(const std::filesystem::path& keep) {
    const auto root = keep.parent_path().parent_path();
    std::error_code error;
    for (const auto& entry : std::filesystem::directory_iterator(root, error)) {
        if (error) break;
        if (entry.path() == keep.parent_path()) {
            for (const auto& item : std::filesystem::directory_iterator(entry.path(), error)) {
                if (error) break;
                if (item.path().extension() == L".part") {
                    if (reparse_point(item.path())) DeleteFileW(item.path().c_str());
                    else std::filesystem::remove(item.path(), error);
                }
            }
            continue;
        }
        if (reparse_point(entry.path())) {
            if (entry.is_directory(error)) RemoveDirectoryW(entry.path().c_str());
            else DeleteFileW(entry.path().c_str());
        } else {
            std::filesystem::remove_all(entry.path(), error);
        }
        error.clear();
    }
}

class Sha256Stream {
public:
    Sha256Stream() {
        if (BCryptOpenAlgorithmProvider(&algorithm_, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0) {
            throw UpdateError("cannot initialize SHA-256");
        }
        DWORD bytes = 0;
        ULONG written = 0;
        if (BCryptGetProperty(algorithm_, BCRYPT_OBJECT_LENGTH,
                              reinterpret_cast<PUCHAR>(&bytes), sizeof(bytes), &written, 0) < 0) {
            throw UpdateError("cannot initialize SHA-256");
        }
        object_.resize(bytes);
        if (BCryptCreateHash(algorithm_, &hash_, object_.data(), bytes, nullptr, 0, 0) < 0) {
            throw UpdateError("cannot initialize SHA-256");
        }
    }
    ~Sha256Stream() {
        if (hash_) BCryptDestroyHash(hash_);
        if (algorithm_) BCryptCloseAlgorithmProvider(algorithm_, 0);
    }
    void add(std::span<const std::uint8_t> data) {
        if (!data.empty() && BCryptHashData(hash_, const_cast<PUCHAR>(data.data()),
                                            static_cast<ULONG>(data.size()), 0) < 0) {
            throw UpdateError("cannot calculate SHA-256");
        }
    }
    std::wstring finish() {
        std::array<std::uint8_t, 32> digest{};
        if (BCryptFinishHash(hash_, digest.data(), static_cast<ULONG>(digest.size()), 0) < 0) {
            throw UpdateError("cannot finish SHA-256");
        }
        return hex_lower(digest);
    }
private:
    BCRYPT_ALG_HANDLE algorithm_{};
    BCRYPT_HASH_HANDLE hash_{};
    std::vector<std::uint8_t> object_;
};

std::pair<std::uint64_t, std::wstring> digest_file(const std::filesystem::path& path,
                                                   std::stop_token stop = {},
                                                   UpdateProgress progress = {},
                                                   std::uint64_t expected = 0) {
    UniqueHandle file(CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ,
                                  nullptr, OPEN_EXISTING,
                                  FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr));
    if (!file) throw_last_error("open downloaded update");
    Sha256Stream digest;
    std::vector<std::uint8_t> buffer(256 * 1024);
    std::uint64_t total = 0;
    while (true) {
        if (stop.stop_requested()) throw UpdateError("update operation cancelled");
        DWORD read = 0;
        if (!ReadFile(file.get(), buffer.data(), static_cast<DWORD>(buffer.size()), &read, nullptr)) {
            throw_last_error("read downloaded update");
        }
        if (!read) break;
        total += read;
        if (expected && total > expected) throw UpdateError("installer size does not match manifest");
        digest.add(std::span(buffer.data(), read));
        if (progress && expected) progress(total, expected);
    }
    return {total, digest.finish()};
}

void verify_file(const std::filesystem::path& path, const UpdateArtifact& artifact,
                 std::stop_token stop = {}, UpdateProgress progress = {}) {
    require_plain_path(path, L"安装包");
    const auto [size, digest] = digest_file(path, stop, std::move(progress), artifact.size);
    if (size != artifact.size) throw UpdateError("installer size does not match manifest");
    if (lower_ascii(digest) != lower_ascii(artifact.sha256)) {
        throw UpdateError("installer SHA-256 verification failed");
    }
}

void mark_internet(const std::filesystem::path& path, std::wstring_view source) {
    const auto stream_path = path.wstring() + L":Zone.Identifier";
    UniqueHandle stream(CreateFileW(stream_path.c_str(), GENERIC_WRITE, 0, nullptr,
                                    CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr));
    if (!stream) throw_last_error("write Mark-of-the-Web");
    const auto data = wide_to_utf8(L"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=" +
                                   std::wstring(source) + L"\r\n");
    DWORD written = 0;
    if (!WriteFile(stream.get(), data.data(), static_cast<DWORD>(data.size()), &written, nullptr) ||
        written != data.size() || !FlushFileBuffers(stream.get())) {
        throw_last_error("write Mark-of-the-Web");
    }
}

void validate_info(const UpdateInfo& info, const Repository& repository, bool require_newer = true) {
    const auto parsed = parse_semver(info.version);
    if (require_newer && compare_semver(parsed, parse_semver(native_version)) <= 0) {
        throw UpdateError("update version is not newer than this build");
    }
    const auto expected_release = repository.url() + L"/releases/tag/v" + info.version;
    const auto expected_name = L"ScreenTranslate-" + info.version + L"-setup-x64.exe";
    const auto expected_url = repository.url() + L"/releases/download/v" + info.version + L"/" + expected_name;
    if (info.release_url != expected_release || info.artifact.name != expected_name ||
        info.artifact.url != expected_url || info.artifact.size == 0 ||
        info.artifact.size > maximum_update_size || !valid_sha256(info.artifact.sha256)) {
        throw UpdateError("update information failed repository binding validation");
    }
}

std::wstring quote_argument(std::wstring_view value) {
    if (value.find_first_of(L" \t\"") == std::wstring_view::npos) return std::wstring(value);
    std::wstring output = L"\"";
    std::size_t slashes = 0;
    for (const wchar_t ch : value) {
        if (ch == L'\\') { ++slashes; continue; }
        if (ch == L'\"') {
            output.append(slashes * 2 + 1, L'\\');
            output.push_back(L'\"');
        } else {
            output.append(slashes, L'\\');
            output.push_back(ch);
        }
        slashes = 0;
    }
    output.append(slashes * 2, L'\\');
    output.push_back(L'\"');
    return output;
}

std::vector<std::wstring> arguments() {
    int count = 0;
    LPWSTR* raw = CommandLineToArgvW(GetCommandLineW(), &count);
    if (!raw) throw_last_error("parse command line");
    std::vector<std::wstring> output;
    for (int index = 1; index < count; ++index) output.emplace_back(raw[index]);
    LocalFree(raw);
    return output;
}

std::map<std::wstring, std::wstring, std::less<>> helper_options() {
    const auto args = arguments();
    if (std::count(args.begin(), args.end(), helper_flag) != 1) {
        throw UpdateError("update helper arguments are invalid");
    }
    const std::set<std::wstring> allowed{
        L"--update-path", L"--update-version", L"--update-size", L"--update-sha256",
        L"--update-repository", L"--parent-pid",
    };
    std::map<std::wstring, std::wstring, std::less<>> output;
    auto iterator = std::find(args.begin(), args.end(), helper_flag) + 1;
    while (iterator != args.end()) {
        const auto key = *iterator++;
        if (!allowed.contains(key) || iterator == args.end() || output.contains(key)) {
            throw UpdateError("update helper arguments are invalid");
        }
        output.emplace(key, *iterator++);
    }
    if (output.size() != allowed.size()) throw UpdateError("update helper arguments are incomplete");
    return output;
}

std::uint64_t unsigned_value(std::wstring_view value, std::wstring_view label) {
    if (value.empty() || !std::all_of(value.begin(), value.end(), [](wchar_t ch) {
            return ch >= L'0' && ch <= L'9';
        })) {
        throw UpdateError(wide_to_utf8(std::wstring(label) + L"无效"));
    }
    try { return std::stoull(std::wstring(value)); }
    catch (...) { throw UpdateError(wide_to_utf8(std::wstring(label) + L"无效")); }
}

}  // namespace

std::optional<UpdateInfo> check_for_update(
    std::wstring_view manifest_url, std::wstring_view repository_url,
    std::wstring_view channel, std::stop_token stop) {
    const auto repository = parse_repository(repository_url);
    if (!same_repository(repository, parse_manifest_repository(manifest_url))) {
        throw UpdateError("manifest URL does not belong to the embedded repository");
    }
    if (channel != L"stable" && channel != L"preview") throw UpdateError("update channel is invalid");
    HttpClient client;
    const auto response = github_get(client, std::wstring(manifest_url), manifest_limit, stop);
    auto info = parse_manifest(response.utf8(), repository);
    if (channel == L"stable" && info.channel != L"stable") return std::nullopt;
    return compare_semver(parse_semver(info.version), parse_semver(native_version)) > 0
        ? std::optional<UpdateInfo>(std::move(info)) : std::nullopt;
}

std::filesystem::path download_update(
    const UpdateInfo& info, std::wstring_view repository_url,
    std::stop_token stop, UpdateProgress progress) {
    const auto repository = parse_repository(repository_url);
    validate_info(info, repository);
    const auto target = target_path(info, true);
    prune_cache(target);
    if (std::filesystem::is_regular_file(target) && !reparse_point(target)) {
        try {
            verify_file(target, info.artifact, stop, progress);
            mark_internet(target, info.artifact.url);
            return target;
        } catch (const UpdateError&) {
            std::error_code ignored;
            std::filesystem::remove(target, ignored);
        }
    }

    const auto part = target.parent_path() /
        (info.artifact.name + L"." + new_uuid() + L".part");
    UniqueHandle file(CreateFileW(part.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW,
                                  FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr));
    if (!file) throw_last_error("create update download");
    std::uint64_t received = 0;
    try {
        HttpClient client;
        github_get(client, info.artifact.url, static_cast<std::size_t>(info.artifact.size), stop,
            [&](std::span<const std::uint8_t> chunk, DWORD status) {
                if (status < 200 || status >= 300 || chunk.empty()) return;
                if (stop.stop_requested()) throw UpdateError("update download cancelled");
                received += chunk.size();
                if (received > info.artifact.size) throw UpdateError("installer is larger than manifest");
                DWORD written = 0;
                if (!WriteFile(file.get(), chunk.data(), static_cast<DWORD>(chunk.size()), &written, nullptr) ||
                    written != chunk.size()) throw_last_error("save update download");
                if (progress) progress(received, info.artifact.size);
            });
        if (received != info.artifact.size) throw UpdateError("installer size does not match manifest");
        if (!FlushFileBuffers(file.get())) throw_last_error("flush update download");
        file.reset();
        verify_file(part, info.artifact, stop);
        if (!MoveFileExW(part.c_str(), target.c_str(),
                         MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
            throw_last_error("commit update download");
        }
        mark_internet(target, info.artifact.url);
        return target;
    } catch (...) {
        file.reset();
        DeleteFileW(part.c_str());
        throw;
    }
}

std::filesystem::path verify_downloaded_installer(
    const std::filesystem::path& path, const UpdateInfo& info,
    std::wstring_view repository_url) {
    const auto repository = parse_repository(repository_url);
    validate_info(info, repository);
    const auto expected = target_path(info, false);
    require_plain_path(expected.parent_path(), L"版本缓存目录");
    require_plain_path(path, L"安装包");
    std::error_code error;
    if (std::filesystem::weakly_canonical(path, error) !=
        std::filesystem::weakly_canonical(expected, error) || error ||
        !std::filesystem::is_regular_file(path)) {
        throw UpdateError("installer is outside the trusted update cache");
    }
    verify_file(path, info.artifact);
    return expected;
}

bool launch_update_helper(
    const std::filesystem::path& installer, const UpdateInfo& info,
    std::wstring_view repository_url, DWORD parent_process_id) {
    const auto verified = verify_downloaded_installer(installer, info, repository_url);
    std::wstring executable(32768, L'\0');
    const DWORD length = GetModuleFileNameW(nullptr, executable.data(), static_cast<DWORD>(executable.size()));
    if (!length || length >= executable.size()) throw_last_error("read executable path");
    executable.resize(length);
    const std::vector<std::wstring> args{
        executable, helper_flag,
        L"--update-path", verified.wstring(),
        L"--update-version", info.version,
        L"--update-size", std::to_wstring(info.artifact.size),
        L"--update-sha256", info.artifact.sha256,
        L"--update-repository", std::wstring(repository_url),
        L"--parent-pid", std::to_wstring(parent_process_id),
    };
    std::wstring command;
    for (const auto& argument : args) {
        if (!command.empty()) command.push_back(L' ');
        command += quote_argument(argument);
    }
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                        CREATE_NEW_PROCESS_GROUP, nullptr, nullptr, &startup, &process)) {
        throw_last_error("start update helper");
    }
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return true;
}

bool is_update_helper_request() {
    const auto args = arguments();
    return std::find(args.begin(), args.end(), helper_flag) != args.end();
}

int run_update_helper() {
    const auto values = helper_options();
    const auto parent = unsigned_value(values.at(L"--parent-pid"), L"父进程编号");
    if (!parent || parent > MAXDWORD) throw UpdateError("parent process ID is invalid");
    UniqueHandle process(OpenProcess(SYNCHRONIZE, FALSE, static_cast<DWORD>(parent)));
    if (!process) {
        const DWORD error = GetLastError();
        if (error != ERROR_INVALID_PARAMETER) throw_last_error("wait for old version", error);
    } else {
        const DWORD waited = WaitForSingleObject(process.get(), 30'000);
        if (waited == WAIT_TIMEOUT) throw UpdateError("old version did not exit in time");
        if (waited != WAIT_OBJECT_0) throw_last_error("wait for old version");
    }
    const auto repository = parse_repository(values.at(L"--update-repository"));
    UpdateInfo info;
    info.version = values.at(L"--update-version");
    const auto semver = parse_semver(info.version);
    info.channel = semver.prerelease.empty() ? L"stable" : L"preview";
    info.release_url = repository.url() + L"/releases/tag/v" + info.version;
    info.artifact.name = L"ScreenTranslate-" + info.version + L"-setup-x64.exe";
    info.artifact.url = repository.url() + L"/releases/download/v" + info.version + L"/" + info.artifact.name;
    info.artifact.size = unsigned_value(values.at(L"--update-size"), L"安装包大小");
    info.artifact.sha256 = lower_ascii(values.at(L"--update-sha256"));
    const auto installer = verify_downloaded_installer(values.at(L"--update-path"), info, repository.url());
    mark_internet(installer, info.artifact.url);
    const auto result = reinterpret_cast<INT_PTR>(
        ShellExecuteW(nullptr, L"open", installer.c_str(), nullptr, installer.parent_path().c_str(), SW_SHOWNORMAL));
    if (result <= 32) throw UpdateError("Windows could not start the installer");
    return 0;
}

void updater_self_test() {
    if (compare_semver(parse_semver(L"1.2.4"), parse_semver(L"1.2.3")) <= 0 ||
        compare_semver(parse_semver(L"1.2.4-rc.1"), parse_semver(L"1.2.4")) >= 0 ||
        compare_semver(parse_semver(L"2.0.0+build.7"), parse_semver(L"1.99.99")) <= 0) {
        throw UpdateError("SemVer self-test failed");
    }
    bool rejected = false;
    try { (void)parse_semver(L"1.2.3-"); } catch (const UpdateError&) { rejected = true; }
    if (!rejected) throw UpdateError("invalid SemVer was accepted");

    const Repository repository{L"reevebyte", L"ScreenTranslate"};
    constexpr std::string_view manifest = R"json({
      "schema_version":2,
      "product":"ScreenTranslate",
      "version":"1.0.3",
      "channel":"stable",
      "published_at":"2026-07-29T12:00:00Z",
      "release_url":"https://github.com/reevebyte/ScreenTranslate/releases/tag/v1.0.3",
      "platform":{"os":"windows","arch":"x86_64","minimum":"10"},
      "artifact":{
        "name":"ScreenTranslate-1.0.3-setup-x64.exe",
        "url":"https://github.com/reevebyte/ScreenTranslate/releases/download/v1.0.3/ScreenTranslate-1.0.3-setup-x64.exe",
        "size":123456,
        "sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
      }
    })json";
    const auto parsed = parse_manifest(manifest, repository);
    validate_info(parsed, repository, false);
    if (parsed.version != L"1.0.3" || parsed.artifact.size != 123456) {
        throw UpdateError("manifest self-test failed");
    }
}

}  // namespace screentrans
