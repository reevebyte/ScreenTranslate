#pragma once

#include <winrt/Windows.Data.Json.h>
#include <winrt/Windows.Foundation.Collections.h>

// wingdi.h maps GetObject to GetObjectW. That macro would also rewrite the
// C++/WinRT IJsonValue::GetObject method name.
#ifdef GetObject
#undef GetObject
#endif

#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

namespace screentrans {

class ConfigStore {
public:
    ConfigStore();

    void load();
    void save() const;

    [[nodiscard]] std::wstring string(std::wstring_view path,
                                      std::wstring_view fallback = {}) const;
    [[nodiscard]] bool boolean(std::wstring_view path, bool fallback = false) const;
    [[nodiscard]] int integer(std::wstring_view path, int fallback = 0) const;
    [[nodiscard]] std::vector<std::wstring> strings(std::wstring_view path) const;
    [[nodiscard]] winrt::Windows::Data::Json::JsonObject object(
        std::wstring_view path) const;

    void set_string(std::wstring_view path, std::wstring_view value);
    void set_boolean(std::wstring_view path, bool value);
    void set_integer(std::wstring_view path, int value);
    void set_strings(std::wstring_view path, const std::vector<std::wstring>& values);

    [[nodiscard]] const winrt::Windows::Data::Json::JsonObject& root() const noexcept {
        return root_;
    }
    [[nodiscard]] const std::filesystem::path& path() const noexcept { return path_; }

    static winrt::Windows::Data::Json::JsonObject defaults();

private:
    winrt::Windows::Data::Json::IJsonValue lookup(std::wstring_view path) const;
    winrt::Windows::Data::Json::JsonObject parent_for(std::wstring_view path,
                                                       std::wstring& leaf);

    std::filesystem::path path_;
    winrt::Windows::Data::Json::JsonObject root_{nullptr};
};

}  // namespace screentrans
