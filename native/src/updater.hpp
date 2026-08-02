#pragma once

#include "http.hpp"
#include "version.hpp"

#include <windows.h>

#include <cstdint>
#include <filesystem>
#include <functional>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>

namespace screentrans {

inline constexpr std::uint64_t maximum_update_size = 512ULL * 1024 * 1024;

class UpdateError : public AppError {
public:
    using AppError::AppError;
};

struct UpdateArtifact {
    std::wstring name;
    std::wstring url;
    std::uint64_t size{};
    std::wstring sha256;
};

struct UpdateInfo {
    std::wstring version;
    std::wstring channel;
    std::wstring release_url;
    std::wstring published_at;
    UpdateArtifact artifact;
};

using UpdateProgress = std::function<void(std::uint64_t, std::uint64_t)>;

std::optional<UpdateInfo> check_for_update(
    std::wstring_view manifest_url,
    std::wstring_view repository_url,
    std::wstring_view channel,
    std::stop_token stop = {});

std::filesystem::path download_update(
    const UpdateInfo& info,
    std::wstring_view repository_url,
    std::stop_token stop = {},
    UpdateProgress progress = {});

std::filesystem::path verify_downloaded_installer(
    const std::filesystem::path& path,
    const UpdateInfo& info,
    std::wstring_view repository_url);

bool launch_update_helper(
    const std::filesystem::path& installer,
    const UpdateInfo& info,
    std::wstring_view repository_url,
    DWORD parent_process_id);

bool is_update_helper_request();
int run_update_helper();
void updater_self_test();

}  // namespace screentrans
