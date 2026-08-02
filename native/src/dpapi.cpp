#include "dpapi.hpp"

#include "util.hpp"

#include <wincrypt.h>

#include <span>
#include <vector>

namespace screentrans::dpapi {

namespace {

class LocalBlob {
public:
    DATA_BLOB value{};
    ~LocalBlob() {
        if (value.pbData) {
            LocalFree(value.pbData);
        }
    }
};

}  // namespace

std::string encrypt(std::string_view plaintext) {
    if (plaintext.empty() || plaintext.starts_with(prefix)) {
        return std::string(plaintext);
    }
    DATA_BLOB input{
        static_cast<DWORD>(plaintext.size()),
        reinterpret_cast<BYTE*>(const_cast<char*>(plaintext.data())),
    };
    LocalBlob output;
    if (!CryptProtectData(&input, L"ScreenTranslate API key", nullptr, nullptr, nullptr,
                          CRYPTPROTECT_UI_FORBIDDEN, &output.value)) {
        throw_last_error("Windows DPAPI encryption failed");
    }
    const auto bytes = std::span<const std::uint8_t>(output.value.pbData, output.value.cbData);
    return std::string(prefix) + base64_encode(bytes);
}

std::string decrypt(std::string_view protected_value) {
    if (!protected_value.starts_with(prefix)) {
        return std::string(protected_value);
    }
    try {
        auto encrypted = base64_decode(protected_value.substr(prefix.size()));
        DATA_BLOB input{static_cast<DWORD>(encrypted.size()), encrypted.data()};
        LocalBlob output;
        if (!CryptUnprotectData(&input, nullptr, nullptr, nullptr, nullptr,
                                CRYPTPROTECT_UI_FORBIDDEN, &output.value)) {
            return {};
        }
        return std::string(reinterpret_cast<const char*>(output.value.pbData), output.value.cbData);
    } catch (...) {
        return {};
    }
}

}  // namespace screentrans::dpapi
