#pragma once

#include "types.hpp"

#include <stop_token>
#include <string>
#include <vector>

namespace screentrans {

class OcrError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

std::vector<std::wstring> available_windows_ocr_languages();
std::vector<OcrLine> recognize_windows_ocr(
    const PixelBuffer& image,
    const std::vector<std::wstring>& languages,
    bool upscale,
    std::stop_token stop = {});

}  // namespace screentrans
