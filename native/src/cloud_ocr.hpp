#pragma once

#include "http.hpp"
#include "types.hpp"

#include <winrt/Windows.Data.Json.h>

#include <stop_token>
#include <vector>

namespace screentrans {

std::vector<OcrLine> recognize_azure_vision(
    HttpClient& http,
    const PixelBuffer& image,
    const winrt::Windows::Data::Json::JsonObject& options,
    std::stop_token stop = {});

std::vector<OcrLine> recognize_youdao_cloud(
    HttpClient& http,
    const PixelBuffer& image,
    std::stop_token stop = {});

}  // namespace screentrans
