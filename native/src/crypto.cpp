#include "crypto.hpp"

#include "util.hpp"

#include <bcrypt.h>

#include <limits>

namespace screentrans {

namespace {

class Algorithm {
public:
    Algorithm(const wchar_t* name, ULONG flags) {
        const NTSTATUS status = BCryptOpenAlgorithmProvider(&value_, name, nullptr, flags);
        if (status < 0) {
            throw AppError("BCryptOpenAlgorithmProvider failed");
        }
    }
    ~Algorithm() { if (value_) BCryptCloseAlgorithmProvider(value_, 0); }
    operator BCRYPT_ALG_HANDLE() const noexcept { return value_; }

private:
    BCRYPT_ALG_HANDLE value_{};
};

class Hash {
public:
    ~Hash() { if (value_) BCryptDestroyHash(value_); }
    BCRYPT_HASH_HANDLE* put() noexcept { return &value_; }
    operator BCRYPT_HASH_HANDLE() const noexcept { return value_; }

private:
    BCRYPT_HASH_HANDLE value_{};
};

ULONG property(BCRYPT_ALG_HANDLE algorithm, const wchar_t* name) {
    ULONG value = 0;
    ULONG written = 0;
    if (BCryptGetProperty(algorithm, name, reinterpret_cast<PUCHAR>(&value),
                          sizeof(value), &written, 0) < 0 || written != sizeof(value)) {
        throw AppError("BCryptGetProperty failed");
    }
    return value;
}

std::vector<std::uint8_t> hash(const wchar_t* algorithm_name,
                               std::span<const std::uint8_t> data,
                               std::span<const std::uint8_t> secret = {}) {
    if (data.size() > std::numeric_limits<ULONG>::max() ||
        secret.size() > std::numeric_limits<ULONG>::max()) {
        throw AppError("hash input is too large");
    }
    Algorithm algorithm(algorithm_name, secret.empty() ? 0 : BCRYPT_ALG_HANDLE_HMAC_FLAG);
    const ULONG object_length = property(algorithm, BCRYPT_OBJECT_LENGTH);
    const ULONG hash_length = property(algorithm, BCRYPT_HASH_LENGTH);
    std::vector<std::uint8_t> object(object_length);
    Hash handle;
    if (BCryptCreateHash(algorithm, handle.put(), object.data(), object_length,
                         secret.empty() ? nullptr : const_cast<PUCHAR>(secret.data()),
                         static_cast<ULONG>(secret.size()), 0) < 0) {
        throw AppError("BCryptCreateHash failed");
    }
    if (!data.empty() &&
        BCryptHashData(handle, const_cast<PUCHAR>(data.data()),
                       static_cast<ULONG>(data.size()), 0) < 0) {
        throw AppError("BCryptHashData failed");
    }
    std::vector<std::uint8_t> output(hash_length);
    if (BCryptFinishHash(handle, output.data(), hash_length, 0) < 0) {
        throw AppError("BCryptFinishHash failed");
    }
    return output;
}

}  // namespace

std::vector<std::uint8_t> md5(std::span<const std::uint8_t> data) {
    return hash(BCRYPT_MD5_ALGORITHM, data);
}

std::vector<std::uint8_t> sha256(std::span<const std::uint8_t> data) {
    return hash(BCRYPT_SHA256_ALGORITHM, data);
}

std::vector<std::uint8_t> hmac_sha256(std::span<const std::uint8_t> key,
                                      std::span<const std::uint8_t> data) {
    if (key.empty()) {
        throw AppError("HMAC key is empty");
    }
    return hash(BCRYPT_SHA256_ALGORITHM, data, key);
}

std::wstring hex_lower(std::span<const std::uint8_t> data) {
    constexpr wchar_t digits[] = L"0123456789abcdef";
    std::wstring output;
    output.reserve(data.size() * 2);
    for (const auto byte : data) {
        output.push_back(digits[byte >> 4]);
        output.push_back(digits[byte & 0x0F]);
    }
    return output;
}

}  // namespace screentrans
