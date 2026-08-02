#pragma once

#include <string>
#include <string_view>

namespace screentrans::dpapi {

inline constexpr std::string_view prefix = "dpapi:v1:";

std::string encrypt(std::string_view plaintext);
std::string decrypt(std::string_view protected_value);

}  // namespace screentrans::dpapi
