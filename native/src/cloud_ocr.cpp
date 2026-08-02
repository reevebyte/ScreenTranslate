#include "cloud_ocr.hpp"

#include "crypto.hpp"
#include "image_codec.hpp"
#include "ocr.hpp"

#include <winrt/Windows.Foundation.Collections.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cwctype>
#include <random>
#include <sstream>
#include <thread>

namespace screentrans {

using namespace winrt::Windows::Data::Json;

namespace {

std::wstring option_string(const JsonObject& object, std::wstring_view key) {
    try {
        return object.GetNamedString(key, L"").c_str();
    } catch (...) {
        return {};
    }
}

JsonObject parse_object(const HttpResponse& response, std::wstring_view label) {
    try {
        return JsonObject::Parse(response.text());
    } catch (...) {
        throw OcrError(wide_to_utf8(label) + " returned invalid JSON");
    }
}

RectF polygon_bounds(const JsonArray& points) {
    if (points.Size() < 8 || points.Size() % 2 != 0) {
        throw OcrError("cloud OCR returned an invalid polygon");
    }
    float left = std::numeric_limits<float>::max();
    float top = std::numeric_limits<float>::max();
    float right = std::numeric_limits<float>::lowest();
    float bottom = std::numeric_limits<float>::lowest();
    for (std::uint32_t index = 0; index + 1 < points.Size(); index += 2) {
        const float x = static_cast<float>(points.GetNumberAt(index));
        const float y = static_cast<float>(points.GetNumberAt(index + 1));
        left = std::min(left, x);
        top = std::min(top, y);
        right = std::max(right, x);
        bottom = std::max(bottom, y);
    }
    if (!std::isfinite(left) || right <= left || bottom <= top) {
        throw OcrError("cloud OCR returned invalid bounds");
    }
    return {left, top, right - left, bottom - top};
}

float weighted_confidence(const std::vector<OcrWord>& words) {
    double total = 0.0;
    double weight = 0.0;
    for (const auto& word : words) {
        if (word.confidence < 0.0F) continue;
        const auto characters = std::max<std::size_t>(1,
            std::count_if(word.text.begin(), word.text.end(), [](wchar_t ch) {
                return !std::iswspace(ch);
            }));
        total += word.confidence * characters;
        weight += characters;
    }
    return weight == 0.0 ? -1.0F : static_cast<float>(total / weight);
}

std::vector<OcrLine> parse_azure_result(const JsonObject& root) {
    std::vector<OcrLine> output;
    const auto pages = root.GetNamedObject(L"analyzeResult").GetNamedArray(L"readResults");
    for (std::uint32_t page_index = 0; page_index < pages.Size(); ++page_index) {
        const auto lines = pages.GetObjectAt(page_index).GetNamedArray(L"lines");
        for (std::uint32_t line_index = 0; line_index < lines.Size(); ++line_index) {
            const auto source = lines.GetObjectAt(line_index);
            OcrLine line;
            line.text = source.GetNamedString(L"text", L"").c_str();
            if (source.HasKey(L"boundingBox")) {
                line.bounds = polygon_bounds(source.GetNamedArray(L"boundingBox"));
            }
            if (source.HasKey(L"words")) {
                const auto words = source.GetNamedArray(L"words");
                for (std::uint32_t word_index = 0; word_index < words.Size(); ++word_index) {
                    const auto item = words.GetObjectAt(word_index);
                    OcrWord word;
                    word.text = item.GetNamedString(L"text", L"").c_str();
                    if (item.HasKey(L"boundingBox")) {
                        word.bounds = polygon_bounds(item.GetNamedArray(L"boundingBox"));
                    }
                    if (item.HasKey(L"confidence")) {
                        word.confidence = static_cast<float>(item.GetNamedNumber(L"confidence"));
                    }
                    line.words.push_back(std::move(word));
                }
            }
            line.confidence = weighted_confidence(line.words);
            if (!trim(line.text).empty() && !line.bounds.empty()) {
                output.push_back(std::move(line));
            }
        }
    }
    return output;
}

std::wstring make_salt() {
    std::random_device random;
    const std::uint64_t upper = static_cast<std::uint64_t>(random()) << 32;
    const std::uint64_t lower = random();
    const auto value = (upper | lower) % 1000000000000000000ULL;
    wchar_t buffer[32]{};
    std::swprintf(buffer, std::size(buffer), L"0.%018llu",
                  static_cast<unsigned long long>(value));
    std::wstring result(buffer);
    while (result.size() > 2 && result.back() == L'0') result.pop_back();
    return result;
}

std::wstring youdao_signature(std::span<const std::uint8_t> png, std::wstring_view salt) {
    const auto encoded = base64_encode(png);
    const auto digest_source = encoded.substr(0, std::min<std::size_t>(10, encoded.size())) +
        std::to_string(encoded.size()) +
        encoded.substr(encoded.size() > 10 ? encoded.size() - 10 : 0);
    const auto raw = std::string("deskdict") + digest_source + wide_to_utf8(salt) +
                     "VPaHE3kX_vl4BhgYiu2n";
    const auto digest = md5(std::span<const std::uint8_t>(
        reinterpret_cast<const std::uint8_t*>(raw.data()), raw.size()));
    return hex_lower(digest);
}

void append_ascii(std::vector<std::uint8_t>& output, std::string_view value) {
    output.insert(output.end(), value.begin(), value.end());
}

void append_form_field(std::vector<std::uint8_t>& output, std::string_view boundary,
                       std::string_view name, std::string_view value) {
    append_ascii(output, "--" + std::string(boundary) + "\r\n");
    append_ascii(output, "Content-Disposition: form-data; name=\"" + std::string(name) +
                         "\"\r\n\r\n");
    append_ascii(output, value);
    append_ascii(output, "\r\n");
}

RectF parse_youdao_bounds(std::wstring_view raw) {
    std::array<float, 4> values{};
    std::wistringstream stream{std::wstring(raw)};
    std::wstring piece;
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (!std::getline(stream, piece, L',')) {
            throw OcrError("Youdao OCR returned invalid bounds");
        }
        try {
            values[index] = std::stof(trim(piece));
        } catch (...) {
            throw OcrError("Youdao OCR returned invalid bounds");
        }
    }
    if (std::getline(stream, piece, L',') || values[2] <= 0 || values[3] <= 0) {
        throw OcrError("Youdao OCR returned invalid bounds");
    }
    return {values[0], values[1], values[2], values[3]};
}

std::vector<OcrLine> parse_youdao_result(const JsonObject& root) {
    const auto error_code = root.GetNamedString(L"errorCode", L"");
    if (error_code != L"0") {
        throw OcrError("Youdao OCR returned error " + wide_to_utf8(error_code.c_str()));
    }
    std::vector<OcrLine> output;
    const auto regions = root.GetNamedArray(L"resRegions", JsonArray{});
    for (std::uint32_t region_index = 0; region_index < regions.Size(); ++region_index) {
        if (regions.GetAt(region_index).ValueType() != JsonValueType::Object) continue;
        const auto region = regions.GetObjectAt(region_index);
        const auto bounds = parse_youdao_bounds(region.GetNamedString(L"boundingBox", L"").c_str());
        std::vector<std::wstring> texts;
        if (region.HasKey(L"lines")) {
            const auto lines = region.GetNamedArray(L"lines");
            for (std::uint32_t index = 0; index < lines.Size(); ++index) {
                if (lines.GetAt(index).ValueType() != JsonValueType::Object) continue;
                auto text = trim(std::wstring(lines.GetObjectAt(index).GetNamedString(L"text", L"").c_str()));
                if (!text.empty()) texts.push_back(std::move(text));
            }
        }
        if (texts.empty()) {
            std::wistringstream lines(region.GetNamedString(L"context", L"").c_str());
            std::wstring text;
            while (std::getline(lines, text)) {
                text = trim(std::move(text));
                if (!text.empty()) texts.push_back(std::move(text));
            }
        }
        if (texts.empty()) continue;
        const float line_height = bounds.height / static_cast<float>(texts.size());
        for (std::size_t index = 0; index < texts.size(); ++index) {
            OcrLine line;
            line.text = std::move(texts[index]);
            line.bounds = {bounds.x, bounds.y + line_height * index, bounds.width, line_height};
            output.push_back(std::move(line));
        }
    }
    return output;
}

}  // namespace

std::vector<OcrLine> recognize_azure_vision(
    HttpClient& http, const PixelBuffer& image, const JsonObject& options,
    std::stop_token stop) {
    const auto endpoint = validate_api_base_url(option_string(options, L"endpoint"),
                                                 L"Azure Vision endpoint");
    const auto key = trim(option_string(options, L"key"));
    if (key.empty()) throw OcrError("尚未填写 Azure Vision OCR 密钥");
    const auto png = encode_png(image);
    HttpRequest analyze;
    analyze.method = L"POST";
    analyze.url = append_query(endpoint + L"/vision/v3.2/read/analyze",
                               {{L"readingOrder", L"natural"},
                                {L"model-version", L"latest"}});
    analyze.headers = {{L"Ocp-Apim-Subscription-Key", key},
                       {L"Content-Type", L"application/octet-stream"}};
    analyze.body = png;
    analyze.max_response_bytes = 256 * 1024;
    const auto accepted = http.send(analyze, stop);
    require_success(accepted, L"Azure Vision OCR", std::span<const std::wstring>(&key, 1));
    const auto location = validate_same_origin_https(
        accepted.header(L"Operation-Location"), endpoint, L"Azure OCR result URL");
    if (location.empty()) throw OcrError("Azure Vision did not return an operation URL");

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(20);
    while (std::chrono::steady_clock::now() < deadline) {
        if (stop.stop_requested()) throw OcrError("OCR cancelled");
        HttpRequest poll;
        poll.url = location;
        poll.headers = {{L"Ocp-Apim-Subscription-Key", key}};
        const auto response = http.send(poll, stop);
        require_success(response, L"Azure Vision OCR", std::span<const std::wstring>(&key, 1));
        const auto root = parse_object(response, L"Azure Vision OCR");
        const auto status = lower_ascii(root.GetNamedString(L"status", L"").c_str());
        if (status == L"succeeded") return parse_azure_result(root);
        if (status == L"failed") throw OcrError("Azure Vision OCR failed");
        if (status != L"notstarted" && status != L"running") {
            throw OcrError("Azure Vision OCR returned an unknown status");
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(350));
    }
    throw OcrError("Azure Vision OCR timed out");
}

std::vector<OcrLine> recognize_youdao_cloud(
    HttpClient& http, const PixelBuffer& image, std::stop_token stop) {
    const auto png = encode_png(image);
    const auto salt = make_salt();
    const auto boundary = "----ScreenTranslate" + wide_to_utf8(new_uuid());
    std::vector<std::uint8_t> body;
    append_form_field(body, boundary, "clientele", "deskdict");
    append_form_field(body, boundary, "salt", wide_to_utf8(salt));
    append_form_field(body, boundary, "sign", wide_to_utf8(youdao_signature(png, salt)));
    append_form_field(body, boundary, "from", "auto");
    append_form_field(body, boundary, "to", "zh-CHS");
    append_form_field(body, boundary, "isSaveHistory", "false");
    append_form_field(body, boundary, "isSyncSaveHistory", "false");
    append_form_field(body, boundary, "funDesc", "photo_translate");
    append_ascii(body, "--" + boundary + "\r\n");
    append_ascii(body, "Content-Disposition: form-data; name=\"multipartFile\"; "
                       "filename=\"capture.png\"\r\nContent-Type: image/png\r\n\r\n");
    body.insert(body.end(), png.begin(), png.end());
    append_ascii(body, "\r\n--" + boundary + "--\r\n");

    HttpRequest request;
    request.method = L"POST";
    request.url = L"https://ocrtran.youdao.com/ocr/imgtranocr";
    request.headers = {{L"User-Agent", L"Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                                        L"AppleWebKit/537.36 (KHTML, like Gecko) "
                                        L"Chrome/127.0.0.0 Safari/537.36"},
                       {L"Accept", L"*/*"},
                       {L"Content-Type", L"multipart/form-data; boundary=" + utf8_to_wide(boundary)}};
    request.body = std::move(body);
    const auto response = http.send(request, stop);
    require_success(response, L"Youdao cloud OCR");
    return parse_youdao_result(parse_object(response, L"Youdao cloud OCR"));
}

}  // namespace screentrans
