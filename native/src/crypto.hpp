#pragma once

#include <cstdint>
#include <span>
#include <string>
#include <vector>

namespace screentrans {

std::vector<std::uint8_t> md5(std::span<const std::uint8_t> data);
std::vector<std::uint8_t> sha256(std::span<const std::uint8_t> data);
std::vector<std::uint8_t> hmac_sha256(std::span<const std::uint8_t> key,
                                      std::span<const std::uint8_t> data);
std::wstring hex_lower(std::span<const std::uint8_t> data);

}  // namespace screentrans
