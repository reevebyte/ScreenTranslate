#include "ocr.hpp"

#include "image_codec.hpp"
#include "language.hpp"
#include "util.hpp"

#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Globalization.h>
#include <winrt/Windows.Graphics.Imaging.h>
#include <winrt/Windows.Media.Ocr.h>
#include <winrt/Windows.Security.Cryptography.h>
#include <winrt/Windows.Storage.Streams.h>

#include <algorithm>
#include <cmath>
#include <cwctype>
#include <set>

namespace screentrans {

using winrt::Windows::Globalization::Language;
using winrt::Windows::Graphics::Imaging::BitmapAlphaMode;
using winrt::Windows::Graphics::Imaging::BitmapPixelFormat;
using winrt::Windows::Graphics::Imaging::SoftwareBitmap;
using winrt::Windows::Media::Ocr::OcrEngine;
using winrt::Windows::Security::Cryptography::CryptographicBuffer;

namespace {

RectF deskew(RectF value, double degrees, float width, float height) {
    if (degrees == 0.0) {
        return value;
    }
    constexpr double pi = 3.14159265358979323846;
    const double radians = degrees * pi / 180.0;
    const double cosine = std::cos(radians);
    const double sine = std::sin(radians);
    const double center_x = width / 2.0;
    const double center_y = height / 2.0;
    const double offset_x = value.x + value.width / 2.0 - center_x;
    const double offset_y = value.y + value.height / 2.0 - center_y;
    const double new_x = center_x + offset_x * cosine - offset_y * sine;
    const double new_y = center_y + offset_x * sine + offset_y * cosine;
    value.x = static_cast<float>(new_x - value.width / 2.0);
    value.y = static_cast<float>(new_y - value.height / 2.0);
    return value;
}

std::wstring join_words(const std::vector<OcrWord>& words) {
    std::wstring result;
    for (const auto& word : words) {
        if (word.text.empty()) {
            continue;
        }
        if (!result.empty() && !is_cjk(result.back()) && !is_cjk(word.text.front())) {
            result.push_back(L' ');
        }
        result += word.text;
    }
    return result;
}

double token_score(std::wstring_view token) {
    int han = 0;
    int alpha = 0;
    int digits = 0;
    for (const wchar_t value : token) {
        const auto code = static_cast<unsigned>(value);
        han += (code >= 0x3400 && code <= 0x9FFF) ||
               (code >= 0xF900 && code <= 0xFAFF) ? 1 : 0;
        alpha += std::iswalpha(value) ? 1 : 0;
        digits += std::iswdigit(value) ? 1 : 0;
    }
    if (han) {
        return static_cast<double>(han);
    }
    if (!alpha) {
        return static_cast<double>(token.size()) * 0.2;
    }
    double result = alpha;
    if (digits && alpha >= 3) {
        result *= 0.4;
    }
    if (alpha <= 2) {
        result *= 0.6;
    }
    return result;
}

double quality(const std::vector<OcrLine>& lines) {
    double result = 0.0;
    for (const auto& line : lines) {
        std::size_t index = 0;
        while (index < line.text.size()) {
            while (index < line.text.size() && std::iswspace(line.text[index])) {
                ++index;
            }
            if (index >= line.text.size()) {
                break;
            }
            const bool han_run = static_cast<unsigned>(line.text[index]) >= 0x3400 &&
                                 static_cast<unsigned>(line.text[index]) <= 0x9FFF;
            const std::size_t start = index++;
            while (index < line.text.size() && !std::iswspace(line.text[index])) {
                const auto code = static_cast<unsigned>(line.text[index]);
                const bool current_han = (code >= 0x3400 && code <= 0x9FFF) ||
                                         (code >= 0xF900 && code <= 0xFAFF);
                if (current_han != han_run) {
                    break;
                }
                ++index;
            }
            result += token_score(std::wstring_view(line.text).substr(start, index - start));
        }
    }
    return result;
}

SoftwareBitmap make_bitmap(const PixelBuffer& image) {
    auto bytes = winrt::array_view<const std::uint8_t>(image.bgra.data(),
                                                       image.bgra.data() + image.bgra.size());
    auto buffer = CryptographicBuffer::CreateFromByteArray(bytes);
    return SoftwareBitmap::CreateCopyFromBuffer(
        buffer, BitmapPixelFormat::Bgra8, image.width, image.height,
        BitmapAlphaMode::Premultiplied);
}

std::vector<OcrLine> recognize_once(const PixelBuffer& image, std::wstring_view tag) {
    const auto engine = OcrEngine::TryCreateFromLanguage(Language(tag));
    if (!engine) {
        throw OcrError("Windows OCR language is not installed: " + wide_to_utf8(tag));
    }
    auto bitmap = make_bitmap(image);
    auto result = engine.RecognizeAsync(bitmap).get();
    double angle = 0.0;
    if (const auto reference = result.TextAngle()) {
        angle = reference.Value();
    }
    std::vector<OcrLine> output;
    for (const auto& source_line : result.Lines()) {
        OcrLine line;
        float left = std::numeric_limits<float>::max();
        float top = std::numeric_limits<float>::max();
        float right = std::numeric_limits<float>::lowest();
        float bottom = std::numeric_limits<float>::lowest();
        for (const auto& source_word : source_line.Words()) {
            const auto bounds = source_word.BoundingRect();
            OcrWord word;
            word.text = source_word.Text().c_str();
            word.bounds = deskew(
                {bounds.X, bounds.Y, bounds.Width, bounds.Height}, angle,
                static_cast<float>(image.width), static_cast<float>(image.height));
            left = std::min(left, word.bounds.x);
            top = std::min(top, word.bounds.y);
            right = std::max(right, word.bounds.right());
            bottom = std::max(bottom, word.bounds.bottom());
            line.words.push_back(std::move(word));
        }
        if (line.words.empty()) {
            continue;
        }
        line.text = join_words(line.words);
        line.bounds = {left, top, right - left, bottom - top};
        output.push_back(std::move(line));
    }
    bitmap.Close();
    return output;
}

}  // namespace

std::vector<std::wstring> available_windows_ocr_languages() {
    WinrtApartment apartment(winrt::apartment_type::multi_threaded);
    std::vector<std::wstring> output;
    for (const auto& language : OcrEngine::AvailableRecognizerLanguages()) {
        output.emplace_back(language.LanguageTag().c_str());
    }
    return output;
}

std::vector<OcrLine> recognize_windows_ocr(
    const PixelBuffer& image,
    const std::vector<std::wstring>& languages,
    bool upscale,
    std::stop_token stop) {
    if (image.empty()) {
        throw OcrError("OCR image is empty");
    }
    WinrtApartment apartment(winrt::apartment_type::multi_threaded);
    std::vector<std::wstring> pool;
    std::set<std::wstring> seen;
    for (const auto& language : languages) {
        if (!language.empty() && seen.insert(language).second) {
            pool.push_back(language);
            if (pool.size() == 3) {
                break;
            }
        }
    }
    if (pool.empty()) {
        const auto available = OcrEngine::AvailableRecognizerLanguages();
        if (available.Size() != 0) {
            pool.emplace_back(available.GetAt(0).LanguageTag().c_str());
        }
    }
    if (pool.empty()) {
        throw OcrError("Windows has no OCR language pack installed");
    }

    double scale = 1.0;
    const int longest = std::max(image.width, image.height);
    if (upscale && longest < 1600) {
        scale = std::min(3.0, std::max(1.0, 1400.0 / std::max(1, longest)));
    }
    PixelBuffer resized;
    const PixelBuffer* work = &image;
    if (scale > 1.05) {
        resized = resize_bgra(image,
                              std::max(1, static_cast<int>(image.width * scale)),
                              std::max(1, static_cast<int>(image.height * scale)));
        work = &resized;
    } else {
        scale = 1.0;
    }

    std::vector<OcrLine> best;
    double best_score = -1.0;
    std::vector<std::string> errors;
    for (const auto& language : pool) {
        if (stop.stop_requested()) {
            throw OcrError("OCR cancelled");
        }
        try {
            auto lines = recognize_once(*work, language);
            const double score = quality(lines);
            if (score > best_score) {
                best_score = score;
                best = std::move(lines);
            }
        } catch (const std::exception& error) {
            errors.emplace_back(error.what());
        }
    }
    if (best_score < 0.0) {
        throw OcrError(errors.empty() ? "Windows OCR failed" : errors.front());
    }
    if (scale != 1.0) {
        const float inverse = static_cast<float>(1.0 / scale);
        for (auto& line : best) {
            line.bounds.x *= inverse;
            line.bounds.y *= inverse;
            line.bounds.width *= inverse;
            line.bounds.height *= inverse;
            for (auto& word : line.words) {
                word.bounds.x *= inverse;
                word.bounds.y *= inverse;
                word.bounds.width *= inverse;
                word.bounds.height *= inverse;
            }
        }
    }
    return best;
}

}  // namespace screentrans
