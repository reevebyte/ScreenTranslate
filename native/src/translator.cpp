#include "translator.hpp"

#include "crypto.hpp"
#include "language.hpp"

#include <winrt/Windows.Foundation.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cwctype>
#include <functional>
#include <regex>
#include <set>

namespace screentrans {

using namespace winrt::Windows::Data::Json;

namespace {

constexpr wchar_t browser_user_agent[] =
    L"Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    L"AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36";

std::wstring json_string(const JsonObject& object, std::wstring_view name,
                         std::wstring_view fallback = {}) {
    try {
        if (object && object.HasKey(name)) {
            const auto value = object.Lookup(name);
            if (value && value.ValueType() == JsonValueType::String) {
                return value.GetString().c_str();
            }
        }
    } catch (...) {
    }
    return std::wstring(fallback);
}

bool json_bool(const JsonObject& object, std::wstring_view name, bool fallback) {
    try {
        if (object && object.HasKey(name)) {
            const auto value = object.Lookup(name);
            if (value && value.ValueType() == JsonValueType::Boolean) {
                return value.GetBoolean();
            }
        }
    } catch (...) {
    }
    return fallback;
}

JsonObject parse_object(const HttpResponse& response, std::wstring_view label) {
    try {
        return JsonObject::Parse(response.text());
    } catch (...) {
        throw TranslateError(wide_to_utf8(label) + " returned invalid JSON");
    }
}

JsonArray parse_array(const HttpResponse& response, std::wstring_view label) {
    try {
        return JsonArray::Parse(response.text());
    } catch (...) {
        throw TranslateError(wide_to_utf8(label) + " returned invalid JSON");
    }
}

std::wstring mapped_code(std::wstring_view value,
                         std::span<const std::pair<std::wstring_view, std::wstring_view>> mappings) {
    for (const auto& [source, target] : mappings) {
        if (source == value) {
            return std::wstring(target);
        }
    }
    return std::wstring(value);
}

std::size_t unicode_length(std::wstring_view value) {
    std::size_t count = 0;
    for (std::size_t index = 0; index < value.size(); ++index, ++count) {
        if (value[index] >= 0xD800 && value[index] <= 0xDBFF && index + 1 < value.size() &&
            value[index + 1] >= 0xDC00 && value[index + 1] <= 0xDFFF) {
            ++index;
        }
    }
    return count;
}

std::pair<std::size_t, std::size_t> limits(std::wstring_view provider) {
    if (provider == L"microsoft") return {100, 40000};
    if (provider == L"google") return {100, 30000};
    if (provider == L"deepl") return {50, 30000};
    if (provider == L"openai" || provider == L"nvidia" || provider == L"anthropic") {
        return {40, 6000};
    }
    if (provider == L"iciba_free") return {50, 12000};
    return {20, 8000};
}

std::vector<std::wstring> json_string_array(const JsonArray& values,
                                             std::wstring_view label) {
    std::vector<std::wstring> output;
    output.reserve(values.Size());
    for (std::uint32_t index = 0; index < values.Size(); ++index) {
        const auto value = values.GetAt(index);
        if (!value || value.ValueType() != JsonValueType::String) {
            throw TranslateError(wide_to_utf8(label) + " returned a non-string item");
        }
        output.emplace_back(value.GetString().c_str());
    }
    return output;
}

JsonArray make_string_array(const std::vector<std::wstring>& texts) {
    JsonArray output;
    for (const auto& text : texts) {
        output.Append(JsonValue::CreateStringValue(text));
    }
    return output;
}

std::wstring nonempty(std::wstring value, std::wstring_view label) {
    if (trim(value).empty()) {
        throw TranslateError(wide_to_utf8(label) + " returned an empty translation");
    }
    return value;
}

std::vector<std::wstring> microsoft_translate(
    HttpClient& http, const JsonObject& options, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    const auto key = trim(json_string(options, L"key"));
    if (key.empty()) {
        throw TranslateError("尚未填写 Azure 翻译密钥（设置 -> 翻译引擎）");
    }
    const auto endpoint = validate_api_base_url(
        json_string(options, L"endpoint", L"https://api.cognitive.microsofttranslator.com"),
        L"Azure Translator endpoint");
    constexpr std::array codes{
        std::pair{std::wstring_view(L"zh-Hans"), std::wstring_view(L"zh-Hans")},
        std::pair{std::wstring_view(L"zh-Hant"), std::wstring_view(L"zh-Hant")},
        std::pair{std::wstring_view(L"en"), std::wstring_view(L"en")},
    };
    std::vector<std::pair<std::wstring, std::wstring>> query{
        {L"api-version", L"3.0"}, {L"to", mapped_code(target, codes)},
        {L"textType", L"plain"},
    };
    if (source && *source != L"auto" && *source != L"other") {
        query.emplace_back(L"from", *source);
    }
    JsonArray payload;
    for (const auto& text : texts) {
        JsonObject item;
        item.SetNamedValue(L"Text", JsonValue::CreateStringValue(text));
        payload.Append(item);
    }
    HttpRequest request;
    request.method = L"POST";
    request.url = append_query(endpoint + L"/translate", query);
    request.headers = {
        {L"Ocp-Apim-Subscription-Key", key},
        {L"Content-Type", L"application/json; charset=utf-8"},
    };
    const auto region = trim(json_string(options, L"region"));
    if (!region.empty()) {
        request.headers.emplace_back(L"Ocp-Apim-Subscription-Region", region);
    }
    request.body = utf8_body(payload.Stringify().c_str());
    const auto response = http.send(request, stop);
    require_success(response, L"Azure Translator", std::span<const std::wstring>(&key, 1));
    const auto root = parse_array(response, L"Azure Translator");
    std::vector<std::wstring> output;
    for (std::uint32_t index = 0; index < root.Size(); ++index) {
        try {
            const auto item = root.GetObjectAt(index);
            const auto translations = item.GetNamedArray(L"translations");
            output.emplace_back(translations.GetObjectAt(0).GetNamedString(L"text").c_str());
        } catch (...) {
            throw TranslateError("Azure Translator 返回格式异常");
        }
    }
    return output;
}

std::vector<std::wstring> google_translate(
    HttpClient& http, const JsonObject& options, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    const auto key = trim(json_string(options, L"key"));
    if (key.empty()) {
        throw TranslateError("尚未填写 Google Cloud Translation API Key");
    }
    constexpr std::array codes{
        std::pair{std::wstring_view(L"zh-Hans"), std::wstring_view(L"zh-CN")},
        std::pair{std::wstring_view(L"zh-Hant"), std::wstring_view(L"zh-TW")},
        std::pair{std::wstring_view(L"en"), std::wstring_view(L"en")},
    };
    JsonObject payload;
    payload.SetNamedValue(L"q", make_string_array(texts));
    payload.SetNamedValue(L"target", JsonValue::CreateStringValue(mapped_code(target, codes)));
    payload.SetNamedValue(L"format", JsonValue::CreateStringValue(L"text"));
    if (source && *source != L"auto" && *source != L"other") {
        payload.SetNamedValue(L"source", JsonValue::CreateStringValue(mapped_code(*source, codes)));
    }
    HttpRequest request;
    request.method = L"POST";
    request.url = L"https://translation.googleapis.com/language/translate/v2";
    request.headers = {{L"X-Goog-Api-Key", key}, {L"Content-Type", L"application/json"}};
    request.body = utf8_body(payload.Stringify().c_str());
    const auto response = http.send(request, stop);
    require_success(response, L"Google Translator", std::span<const std::wstring>(&key, 1));
    try {
        const auto items = parse_object(response, L"Google Translator")
            .GetNamedObject(L"data").GetNamedArray(L"translations");
        std::vector<std::wstring> output;
        for (std::uint32_t index = 0; index < items.Size(); ++index) {
            output.push_back(html_unescape(
                std::wstring(items.GetObjectAt(index).GetNamedString(L"translatedText").c_str())));
        }
        return output;
    } catch (const TranslateError&) {
        throw;
    } catch (...) {
        throw TranslateError("Google Translator 返回格式异常");
    }
}

std::vector<std::wstring> google_free_translate(
    HttpClient& http, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    constexpr std::array codes{
        std::pair{std::wstring_view(L"zh-Hans"), std::wstring_view(L"zh-CN")},
        std::pair{std::wstring_view(L"zh-Hant"), std::wstring_view(L"zh-TW")},
        std::pair{std::wstring_view(L"en"), std::wstring_view(L"en")},
    };
    std::vector<std::wstring> output;
    for (const auto& text : texts) {
        HttpRequest request;
        request.url = append_query(
            L"https://translate.googleapis.com/translate_a/single",
            {{L"client", L"gtx"},
             {L"sl", source && *source != L"other" ? mapped_code(*source, codes) : L"auto"},
             {L"tl", mapped_code(target, codes)}, {L"dt", L"t"}, {L"q", text}});
        const auto response = http.send(request, stop);
        require_success(response, L"Google free translator");
        try {
            const auto root = parse_array(response, L"Google free translator");
            const auto segments = root.GetArrayAt(0);
            std::wstring translated;
            for (std::uint32_t index = 0; index < segments.Size(); ++index) {
                const auto segment = segments.GetArrayAt(index);
                if (segment.Size() && segment.GetAt(0).ValueType() == JsonValueType::String) {
                    translated += segment.GetStringAt(0).c_str();
                }
            }
            output.push_back(nonempty(std::move(translated), L"Google free translator"));
        } catch (const TranslateError&) {
            throw;
        } catch (...) {
            throw TranslateError("Google 免密翻译返回格式异常");
        }
    }
    return output;
}

std::vector<std::wstring> deepl_translate(
    HttpClient& http, const JsonObject& options, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    const auto key = trim(json_string(options, L"key"));
    if (key.empty()) {
        throw TranslateError("尚未填写 DeepL 密钥");
    }
    const bool free = json_bool(options, L"free_plan", true) || key.ends_with(L":fx");
    const std::wstring host = free ? L"https://api-free.deepl.com" : L"https://api.deepl.com";
    constexpr std::array codes{
        std::pair{std::wstring_view(L"zh-Hans"), std::wstring_view(L"ZH")},
        std::pair{std::wstring_view(L"zh-Hant"), std::wstring_view(L"ZH")},
        std::pair{std::wstring_view(L"en"), std::wstring_view(L"EN-US")},
    };
    JsonObject payload;
    payload.SetNamedValue(L"text", make_string_array(texts));
    auto target_code = mapped_code(target, codes);
    std::transform(target_code.begin(), target_code.end(), target_code.begin(), std::towupper);
    payload.SetNamedValue(L"target_lang", JsonValue::CreateStringValue(target_code));
    if (source && *source != L"auto" && *source != L"other") {
        auto source_code = mapped_code(*source, codes);
        if (const auto dash = source_code.find(L'-'); dash != std::wstring::npos) {
            source_code.resize(dash);
        }
        std::transform(source_code.begin(), source_code.end(), source_code.begin(), std::towupper);
        payload.SetNamedValue(L"source_lang", JsonValue::CreateStringValue(source_code));
    }
    HttpRequest request;
    request.method = L"POST";
    request.url = host + L"/v2/translate";
    request.headers = {{L"Authorization", L"DeepL-Auth-Key " + key},
                       {L"Content-Type", L"application/json"}};
    request.body = utf8_body(payload.Stringify().c_str());
    const auto response = http.send(request, stop);
    require_success(response, L"DeepL", std::span<const std::wstring>(&key, 1));
    try {
        const auto items = parse_object(response, L"DeepL").GetNamedArray(L"translations");
        std::vector<std::wstring> output;
        for (std::uint32_t index = 0; index < items.Size(); ++index) {
            output.emplace_back(items.GetObjectAt(index).GetNamedString(L"text").c_str());
        }
        return output;
    } catch (const TranslateError&) {
        throw;
    } catch (...) {
        throw TranslateError("DeepL 返回格式异常");
    }
}

std::wstring strip_fence(std::wstring value) {
    value = trim(std::move(value));
    if (value.starts_with(L"```")) {
        const auto newline = value.find(L'\n');
        if (newline != std::wstring::npos) {
            value.erase(0, newline + 1);
        }
    }
    value = trim(std::move(value));
    if (value.ends_with(L"```")) {
        value.resize(value.size() - 3);
    }
    return trim(std::move(value));
}

std::optional<std::vector<std::wstring>> parse_ai_array(std::wstring raw,
                                                        std::size_t expected) {
    raw = strip_fence(std::move(raw));
    const auto begin = raw.find(L'[');
    const auto end = raw.rfind(L']');
    if (begin == std::wstring::npos || end == std::wstring::npos || end <= begin) {
        return std::nullopt;
    }
    try {
        const auto values = JsonArray::Parse(raw.substr(begin, end - begin + 1));
        if (values.Size() != expected) {
            return std::nullopt;
        }
        return json_string_array(values, L"AI model");
    } catch (...) {
        return std::nullopt;
    }
}

std::wstring plain_ai(std::wstring raw) {
    raw = strip_fence(std::move(raw));
    try {
        const auto value = JsonValue::Parse(raw);
        if (value.ValueType() == JsonValueType::String) {
            return value.GetString().c_str();
        }
        return {};
    } catch (...) {
        return raw;
    }
}

std::wstring ai_system_prompt(std::wstring_view target, bool retry) {
    const auto language = target_prompt_name(target);
    if (retry) {
        return L"Translate the following JSON array into " + language +
               L".\nThe previous attempt failed because the output was NOT in " + language +
               L".\nEvery returned string must be written in " + language +
               L". Return ONLY a JSON array of strings of the same length. No other text.";
    }
    return L"You are a professional translation engine embedded in a screen-translation tool. "
           L"You will receive a JSON array of text segments extracted from a screenshot.\n"
           L"Rules:\n1. Translate every segment into " + language +
           L". The output text MUST be written in " + language +
           L". Never answer in the source language, and never explain or comment.\n"
           L"2. Return ONLY a JSON array of strings, same length and same order as the input. "
           L"No markdown fences, no commentary, no keys.\n"
           L"3. Keep the translation concise and natural; it will be drawn over limited space.\n"
           L"4. Preserve numbers, code identifiers, URLs, file paths and proper nouns when needed.\n"
           L"5. If a segment is already written in " + language + L", return it unchanged.\n"
           L"6. Never merge or split segments. An empty input maps to an empty string.";
}

bool preservable(std::wstring_view text) {
    std::wstring value = trim(std::wstring(text));
    if (value.empty() || std::any_of(value.begin(), value.end(), [](wchar_t ch) {
            return ch > 127 || std::iswspace(ch);
        })) {
        return false;
    }
    int letters = 0;
    int uppercase = 0;
    bool marked = false;
    for (const wchar_t ch : value) {
        if (std::iswalpha(ch)) {
            ++letters;
            uppercase += std::iswupper(ch) ? 1 : 0;
        }
        marked = marked || std::iswdigit(ch) || ch == L'_' || ch == L'.' || ch == L'/' ||
                 ch == L':' || ch == L'\\' || ch == L'-';
    }
    return letters == 0 || letters == 1 || uppercase == letters || uppercase >= 2 || marked;
}

bool valid_ai_output(std::wstring_view source, std::wstring_view translated,
                     std::wstring_view target) {
    if (trim(std::wstring(translated)).empty()) {
        return false;
    }
    const bool source_has_letters = std::any_of(source.begin(), source.end(), std::iswalpha);
    const bool output_has_letters = std::any_of(translated.begin(), translated.end(), std::iswalpha);
    if (source_has_letters && !output_has_letters) {
        return false;
    }
    if (trim(std::wstring(source)) == trim(std::wstring(translated)) && preservable(source)) {
        return true;
    }
    return matches_target(translated, target);
}

std::wstring normalize_ai_root(std::wstring value) {
    value = trim(std::move(value));
    for (const auto suffix : {std::wstring_view(L"/chat/completions"),
                              std::wstring_view(L"/completions")}) {
        if (value.ends_with(suffix)) {
            value.resize(value.size() - suffix.size());
            break;
        }
    }
    return value;
}

std::wstring source_value(const std::optional<std::wstring>& source) {
    return source && !source->empty() ? *source : L"auto";
}

std::wstring microsoft_free_signature(std::wstring_view request_path) {
    static constexpr std::array<std::uint8_t, 64> private_key{
        0xA2,0x29,0x3A,0x3D,0xD0,0xDD,0x32,0x73,0x97,0x7A,0x64,0xDB,0xC2,0xF3,0x27,0xF5,
        0xD7,0xBF,0x87,0xD9,0x45,0x9D,0xF0,0x5A,0x09,0x66,0xC6,0x30,0xC6,0x6A,0xAA,0x84,
        0x9A,0x41,0xAA,0x94,0x3A,0xA8,0xD5,0x1A,0x6E,0x4D,0xAA,0xC9,0xA3,0x70,0x12,0x35,
        0xC7,0xEB,0x12,0xF6,0xE8,0x23,0x07,0x9E,0x47,0x10,0x95,0x91,0x88,0x55,0xD8,0x17,
    };
    static constexpr std::array weekdays{L"Sun", L"Mon", L"Tue", L"Wed", L"Thu", L"Fri", L"Sat"};
    static constexpr std::array months{L"Jan", L"Feb", L"Mar", L"Apr", L"May", L"Jun",
                                       L"Jul", L"Aug", L"Sep", L"Oct", L"Nov", L"Dec"};
    SYSTEMTIME now{};
    GetSystemTime(&now);
    wchar_t date_buffer[64]{};
    std::swprintf(date_buffer, std::size(date_buffer), L"%ls, %02u %ls %04u %02u:%02u:%02uGMT",
                  weekdays[now.wDayOfWeek], now.wDay,
                  months[std::clamp<int>(now.wMonth - 1, 0, 11)], now.wYear,
                  now.wHour, now.wMinute, now.wSecond);
    auto guid = lower_ascii(new_uuid());
    guid.erase(std::remove(guid.begin(), guid.end(), L'-'), guid.end());
    std::wstring raw = L"MSTranslatorAndroidApp" + url_encode(request_path) +
                       date_buffer + guid;
    raw = lower_ascii(std::move(raw));
    const auto bytes = wide_to_utf8(raw);
    const auto digest = hmac_sha256(private_key,
        std::span<const std::uint8_t>(reinterpret_cast<const std::uint8_t*>(bytes.data()), bytes.size()));
    return L"MSTranslatorAndroidApp::" + utf8_to_wide(base64_encode(digest)) +
           L"::" + date_buffer + L"::" + guid;
}

std::vector<std::wstring> microsoft_free_translate(
    HttpClient& http, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    constexpr std::wstring_view endpoint = L"api.cognitive.microsofttranslator.com";
    std::vector<std::wstring> output;
    for (const auto& text : texts) {
        std::wstring path = std::wstring(endpoint) + L"/translate?api-version=3.0&to=" +
                            url_encode(target);
        if (source && *source != L"auto") path += L"&from=" + url_encode(*source);
        JsonObject item;
        item.SetNamedValue(L"Text", JsonValue::CreateStringValue(text));
        JsonArray body;
        body.Append(item);
        HttpRequest request;
        request.method = L"POST";
        request.url = L"https://" + path;
        request.headers = {{L"X-MT-Signature", microsoft_free_signature(path)},
                           {L"User-Agent", browser_user_agent},
                           {L"Content-Type", L"application/json"}};
        request.body = utf8_body(body.Stringify().c_str());
        const auto response = http.send(request, stop);
        require_success(response, L"Microsoft free translator");
        try {
            const auto root = parse_array(response, L"Microsoft free translator");
            output.push_back(nonempty(root.GetObjectAt(0).GetNamedArray(L"translations")
                .GetObjectAt(0).GetNamedString(L"text").c_str(), L"Microsoft free translator"));
        } catch (const TranslateError&) {
            throw;
        } catch (...) {
            throw TranslateError("微软免密翻译返回格式异常");
        }
    }
    return output;
}

std::wstring tencent_code(std::wstring_view code) {
    if (code == L"zh-Hans") return L"zh";
    if (code == L"zh-Hant") return L"zh-TW";
    return std::wstring(code);
}

std::vector<std::wstring> tencent_free_translate(
    HttpClient& http, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    std::vector<std::wstring> output;
    for (const auto& text : texts) {
        JsonObject header;
        header.SetNamedValue(L"fn", JsonValue::CreateStringValue(L"auto_translation_block"));
        header.SetNamedValue(L"client_key", JsonValue::CreateStringValue(
            L"browser-chrome-110.0.0-Mac OS-df4bd4c5-a65d-44b2-a40f-42f34f3535f2-1677486696487"));
        JsonObject source_object;
        source_object.SetNamedValue(L"lang", JsonValue::CreateStringValue(tencent_code(source_value(source))));
        source_object.SetNamedValue(L"text_block", JsonValue::CreateStringValue(text));
        JsonObject target_object;
        target_object.SetNamedValue(L"lang", JsonValue::CreateStringValue(tencent_code(target)));
        JsonObject payload;
        payload.SetNamedValue(L"header", header);
        payload.SetNamedValue(L"type", JsonValue::CreateStringValue(L"plain"));
        payload.SetNamedValue(L"model_category", JsonValue::CreateStringValue(L"normal"));
        payload.SetNamedValue(L"source", source_object);
        payload.SetNamedValue(L"target", target_object);
        HttpRequest request;
        request.method = L"POST";
        request.url = L"https://transmart.qq.com/api/imt";
        request.headers = {{L"User-Agent", browser_user_agent},
                           {L"Referer", L"https://yi.qq.com/zh-CN/index"},
                           {L"Content-Type", L"application/json"}};
        request.body = utf8_body(payload.Stringify().c_str());
        const auto response = http.send(request, stop);
        require_success(response, L"Tencent translator");
        try {
            const auto value = parse_object(response, L"Tencent translator").Lookup(L"auto_translation");
            std::wstring translated;
            if (value.ValueType() == JsonValueType::String) {
                translated = value.GetString().c_str();
            } else if (value.ValueType() == JsonValueType::Array) {
                const auto items = value.GetArray();
                for (std::uint32_t index = 0; index < items.Size(); ++index) {
                    if (items.GetAt(index).ValueType() == JsonValueType::String) {
                        if (!translated.empty()) translated.push_back(L'\n');
                        translated += items.GetStringAt(index).c_str();
                    }
                }
            }
            output.push_back(nonempty(std::move(translated), L"Tencent translator"));
        } catch (const TranslateError&) {
            throw;
        } catch (...) {
            throw TranslateError("腾讯交互翻译返回格式异常");
        }
    }
    return output;
}

std::wstring yandex_code(std::wstring_view code) {
    return code == L"zh-Hans" || code == L"zh-Hant" ? L"zh" : std::wstring(code);
}

std::vector<std::wstring> yandex_free_translate(
    HttpClient& http, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    std::vector<std::wstring> output;
    for (const auto& text : texts) {
        auto source_code = yandex_code(source_value(source));
        const auto target_code = yandex_code(target);
        const auto language = source_code == L"auto" ? target_code : source_code + L"-" + target_code;
        auto guid = lower_ascii(new_uuid());
        guid.erase(std::remove(guid.begin(), guid.end(), L'-'), guid.end());
        HttpRequest request;
        request.method = L"POST";
        request.url = append_query(L"https://translate.yandex.net/api/v1/tr.json/translate",
                                   {{L"ucid", guid}, {L"srv", L"android"}, {L"format", L"text"}});
        request.headers = {{L"User-Agent", L"ru.yandex.translate/3.20.2024"},
                           {L"Content-Type", L"application/x-www-form-urlencoded"}};
        request.body = form_body({{L"text", text}, {L"lang", language}});
        const auto response = http.send(request, stop);
        require_success(response, L"Yandex translator");
        try {
            const auto values = parse_object(response, L"Yandex translator").GetNamedArray(L"text");
            output.push_back(nonempty(values.Size() ? std::wstring(values.GetStringAt(0).c_str()) : L"",
                                      L"Yandex translator"));
        } catch (const TranslateError&) {
            throw;
        } catch (...) {
            throw TranslateError("Yandex 翻译返回格式异常");
        }
    }
    return output;
}

std::wstring iciba_code(std::wstring_view code) {
    if (code == L"zh-Hans") return L"zh";
    if (code == L"zh-Hant") return L"cht";
    return std::wstring(code);
}

std::vector<std::wstring> iciba_free_translate(
    HttpClient& http, const std::vector<std::wstring>& texts,
    std::wstring_view target, const std::optional<std::wstring>& source,
    std::stop_token stop) {
    constexpr std::wstring_view path = L"/dictionary/fy/batch";
    constexpr std::wstring_view client = L"6";
    constexpr std::wstring_view key = L"1000006";
    constexpr std::wstring_view salt = L"7ece94d9f9c202b0d2ec557dg4r9bc";
    const auto milliseconds = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    const auto timestamp = std::to_wstring(milliseconds);
    const auto sign_text = std::wstring(path) + std::wstring(client) + std::wstring(key) +
                           timestamp + std::wstring(salt);
    const auto sign_utf8 = wide_to_utf8(sign_text);
    const auto digest = md5(std::span<const std::uint8_t>(
        reinterpret_cast<const std::uint8_t*>(sign_utf8.data()), sign_utf8.size()));
    JsonObject payload;
    payload.SetNamedValue(L"from", JsonValue::CreateStringValue(iciba_code(source_value(source))));
    payload.SetNamedValue(L"to", JsonValue::CreateStringValue(iciba_code(target)));
    payload.SetNamedValue(L"textList", make_string_array(texts));
    HttpRequest request;
    request.method = L"POST";
    request.url = append_query(L"https://dictionary.iciba.com" + std::wstring(path),
                               {{L"client", std::wstring(client)}, {L"key", std::wstring(key)},
                                {L"timestamp", timestamp}, {L"signature", hex_lower(digest)}});
    request.headers = {{L"Origin", L"https://www.iciba.com"},
                       {L"Referer", L"https://www.iciba.com/"},
                       {L"User-Agent", browser_user_agent},
                       {L"Content-Type", L"application/json"}};
    request.body = utf8_body(payload.Stringify().c_str());
    const auto response = http.send(request, stop);
    require_success(response, L"Iciba translator");
    try {
        const auto root = parse_object(response, L"Iciba translator");
        const auto code_value = root.Lookup(L"code");
        const bool success = (code_value.ValueType() == JsonValueType::Number && code_value.GetNumber() == 1) ||
                             (code_value.ValueType() == JsonValueType::String && code_value.GetString() == L"1");
        if (!success) throw TranslateError("词霸翻译返回了错误状态");
        const auto items = root.GetNamedArray(L"data");
        std::vector<std::wstring> output;
        for (std::uint32_t index = 0; index < items.Size(); ++index) {
            const auto value = items.GetAt(index);
            std::wstring translated;
            if (value.ValueType() == JsonValueType::Object) {
                translated = value.GetObject().GetNamedString(L"out", L"").c_str();
            } else if (value.ValueType() == JsonValueType::String) {
                translated = value.GetString().c_str();
            }
            output.push_back(nonempty(std::move(translated), L"Iciba translator"));
        }
        return output;
    } catch (const TranslateError&) {
        throw;
    } catch (...) {
        throw TranslateError("词霸翻译返回格式异常");
    }
}

}  // namespace

std::vector<std::wstring> TranslatorService::translate(
    std::wstring_view provider, const JsonObject& options,
    const std::vector<std::wstring>& texts, std::wstring_view target,
    const std::optional<std::wstring>& source, std::stop_token stop) {
    const auto [max_items, max_characters] = limits(provider);
    std::vector<std::wstring> output;
    std::vector<std::wstring> chunk;
    std::size_t characters = 0;
    auto flush = [&] {
        if (chunk.empty()) return;
        auto translated = translate_batch(provider, options, chunk, target, source, stop);
        if (translated.size() != chunk.size()) {
            throw TranslateError("翻译接口返回条数与输入不一致");
        }
        output.insert(output.end(), std::make_move_iterator(translated.begin()),
                      std::make_move_iterator(translated.end()));
        chunk.clear();
        characters = 0;
    };
    for (const auto& text : texts) {
        const auto length = unicode_length(text);
        if (!chunk.empty() && (chunk.size() >= max_items || characters + length > max_characters)) {
            flush();
        }
        if (stop.stop_requested()) {
            throw TranslateError("翻译已取消");
        }
        chunk.push_back(text);
        characters += length;
    }
    flush();
    return output;
}

std::vector<std::wstring> TranslatorService::translate_batch(
    std::wstring_view provider, const JsonObject& options,
    const std::vector<std::wstring>& texts, std::wstring_view target,
    const std::optional<std::wstring>& source, std::stop_token stop) {
    if (provider == L"microsoft") {
        return microsoft_translate(http_, options, texts, target, source, stop);
    }
    if (provider == L"google") {
        return google_translate(http_, options, texts, target, source, stop);
    }
    if (provider == L"google_free") {
        return google_free_translate(http_, texts, target, source, stop);
    }
    if (provider == L"deepl") {
        return deepl_translate(http_, options, texts, target, source, stop);
    }
    if (provider == L"microsoft_free") {
        return microsoft_free_translate(http_, texts, target, source, stop);
    }
    if (provider == L"tencent_free") {
        return tencent_free_translate(http_, texts, target, source, stop);
    }
    if (provider == L"yandex_free") {
        return yandex_free_translate(http_, texts, target, source, stop);
    }
    if (provider == L"iciba_free") {
        return iciba_free_translate(http_, texts, target, source, stop);
    }
    if (provider == L"bing_free") {
        std::vector<std::wstring> output;
        for (const auto& text : texts) {
            auto token = bing_token(stop);
            HttpRequest request;
            request.method = L"POST";
            request.url = append_query(token.host + L"/ttranslatev3",
                                       {{L"isVertical", L"1"}, {L"IG", token.ig}, {L"IID", token.iid}});
            request.headers = {{L"Referer", token.host + L"/translator"},
                               {L"Origin", token.host},
                               {L"Content-Type", L"application/x-www-form-urlencoded"}};
            const auto source_code = source_value(source) == L"auto" ? L"auto-detect" : source_value(source);
            const auto target_code = target == L"auto" ? L"auto-detect" : std::wstring(target);
            request.body = form_body({{L"fromLang", source_code}, {L"to", target_code},
                                      {L"text", text}, {L"token", token.token}, {L"key", token.key}});
            const auto response = http_.send(request, stop);
            if (response.status == 401 || response.status == 403 || response.status == 429) {
                std::lock_guard lock(bing_mutex_);
                bing_token_.reset();
            }
            require_success(response, L"Bing translator");
            try {
                const auto root = parse_array(response, L"Bing translator");
                output.push_back(nonempty(root.GetObjectAt(0).GetNamedArray(L"translations")
                    .GetObjectAt(0).GetNamedString(L"text").c_str(), L"Bing translator"));
            } catch (const TranslateError&) {
                throw;
            } catch (...) {
                std::lock_guard lock(bing_mutex_);
                bing_token_.reset();
                throw TranslateError("必应翻译返回格式已经变化");
            }
        }
        return output;
    }
    if (provider == L"openai" || provider == L"nvidia" || provider == L"anthropic") {
        return translate_ai(provider, options, texts, target, stop);
    }
    throw TranslateError("未知的翻译引擎：" + wide_to_utf8(provider));
}

TranslatorService::BingToken TranslatorService::bing_token(std::stop_token stop) {
    std::lock_guard lock(bing_mutex_);
    if (bing_token_) return *bing_token_;
    auto fetch = [&](std::wstring host) {
        HttpRequest request;
        request.url = host + L"/translator";
        request.headers = {{L"User-Agent", browser_user_agent}};
        request.max_response_bytes = max_text_response;
        const auto response = http_.send(request, stop);
        require_success(response, L"Bing translator");
        const auto html = response.text();
        const std::wregex token_pattern(
            LR"re(params_AbusePreventionHelper\s*=\s*\[\s*([^,\]]+)\s*,\s*"([^"]+)")re");
        const std::wregex ig_pattern(LR"re(IG:"([^"]+)")re");
        const std::wregex iid_pattern(LR"re(data-iid="([^"]+)")re");
        std::wsmatch token_match;
        std::wsmatch ig_match;
        std::wsmatch iid_match;
        if (!std::regex_search(html, token_match, token_pattern) ||
            !std::regex_search(html, ig_match, ig_pattern)) {
            throw TranslateError("必应网页令牌格式已经变化");
        }
        auto key = trim(token_match[1].str());
        if (key.size() >= 2 && key.front() == L'"' && key.back() == L'"') {
            key = key.substr(1, key.size() - 2);
        }
        BingToken value;
        value.key = std::move(key);
        value.token = token_match[2].str();
        value.ig = ig_match[1].str();
        value.iid = std::regex_search(html, iid_match, iid_pattern)
            ? iid_match[1].str()
            : L"translator.5023";
        value.host = std::move(host);
        return value;
    };
    try {
        bing_token_ = fetch(L"https://cn.bing.com");
    } catch (...) {
        bing_token_ = fetch(L"https://www.bing.com");
    }
    return *bing_token_;
}

std::wstring TranslatorService::ai_call(
    std::wstring_view provider, const JsonObject& options,
    std::wstring_view system, std::wstring_view user, std::stop_token stop) {
    const auto key = trim(json_string(options, L"key"));
    if (provider == L"anthropic") {
        if (key.empty()) {
            throw TranslateError("尚未填写 Anthropic API Key");
        }
        const auto base = validate_api_base_url(
            json_string(options, L"base_url", L"https://api.anthropic.com"),
            L"Anthropic endpoint");
        const auto model = trim(json_string(options, L"model", L"claude-sonnet-5"));
        JsonObject payload;
        payload.SetNamedValue(L"model", JsonValue::CreateStringValue(model));
        payload.SetNamedValue(L"max_tokens", JsonValue::CreateNumberValue(4096));
        payload.SetNamedValue(L"temperature", JsonValue::CreateNumberValue(0));
        payload.SetNamedValue(L"system", JsonValue::CreateStringValue(system));
        JsonObject message;
        message.SetNamedValue(L"role", JsonValue::CreateStringValue(L"user"));
        message.SetNamedValue(L"content", JsonValue::CreateStringValue(user));
        JsonArray messages;
        messages.Append(message);
        payload.SetNamedValue(L"messages", messages);
        HttpRequest request;
        request.method = L"POST";
        request.url = base + L"/v1/messages";
        request.headers = {{L"x-api-key", key},
                           {L"anthropic-version", L"2023-06-01"},
                           {L"Content-Type", L"application/json"}};
        request.body = utf8_body(payload.Stringify().c_str());
        const auto response = http_.send(request, stop);
        require_success(response, L"Anthropic", std::span<const std::wstring>(&key, 1));
        try {
            const auto content = parse_object(response, L"Anthropic").GetNamedArray(L"content");
            std::wstring output;
            for (std::uint32_t index = 0; index < content.Size(); ++index) {
                const auto block = content.GetObjectAt(index);
                if (block.GetNamedString(L"type", L"") == L"text") {
                    output += block.GetNamedString(L"text", L"").c_str();
                }
            }
            return output;
        } catch (const TranslateError&) {
            throw;
        } catch (...) {
            throw TranslateError("Anthropic 返回格式异常");
        }
    }

    const auto default_base = provider == L"nvidia"
        ? std::wstring_view(L"https://integrate.api.nvidia.com/v1")
        : std::wstring_view();
    auto base = normalize_ai_root(json_string(options, L"base_url", default_base));
    base = validate_api_base_url(base, L"OpenAI compatible endpoint", !key.empty(), true);
    const auto model = trim(json_string(options, L"model"));
    if (model.empty()) {
        throw TranslateError("尚未选择模型，请先刷新模型列表");
    }
    JsonObject payload;
    payload.SetNamedValue(L"model", JsonValue::CreateStringValue(model));
    payload.SetNamedValue(L"temperature", JsonValue::CreateNumberValue(0));
    payload.SetNamedValue(L"stream", JsonValue::CreateBooleanValue(false));
    JsonArray messages;
    JsonObject system_message;
    system_message.SetNamedValue(L"role", JsonValue::CreateStringValue(L"system"));
    system_message.SetNamedValue(L"content", JsonValue::CreateStringValue(system));
    messages.Append(system_message);
    JsonObject user_message;
    user_message.SetNamedValue(L"role", JsonValue::CreateStringValue(L"user"));
    user_message.SetNamedValue(L"content", JsonValue::CreateStringValue(user));
    messages.Append(user_message);
    payload.SetNamedValue(L"messages", messages);
    HttpRequest request;
    request.method = L"POST";
    request.url = base + L"/chat/completions";
    request.headers = {{L"Content-Type", L"application/json"}};
    if (!key.empty()) {
        request.headers.emplace_back(L"Authorization", L"Bearer " + key);
    }
    request.body = utf8_body(payload.Stringify().c_str());
    const auto response = http_.send(request, stop);
    require_success(response, L"OpenAI compatible API",
                    key.empty() ? std::span<const std::wstring>()
                                : std::span<const std::wstring>(&key, 1));
    try {
        return parse_object(response, L"OpenAI compatible API")
            .GetNamedArray(L"choices").GetObjectAt(0)
            .GetNamedObject(L"message").GetNamedString(L"content", L"").c_str();
    } catch (const TranslateError&) {
        throw;
    } catch (...) {
        throw TranslateError("OpenAI 兼容接口返回格式异常");
    }
}

std::vector<std::wstring> TranslatorService::translate_ai(
    std::wstring_view provider, const JsonObject& options,
    const std::vector<std::wstring>& texts, std::wstring_view target,
    std::stop_token stop) {
    if (texts.empty()) {
        return {};
    }
    auto call = [&](const std::vector<std::wstring>& input, bool retry) {
        return ai_call(provider, options, ai_system_prompt(target, retry),
                       make_string_array(input).Stringify().c_str(), stop);
    };
    auto parsed = parse_ai_array(call(texts, false), texts.size());
    std::vector<std::wstring> output;
    if (parsed) {
        output = std::move(*parsed);
    } else {
        output.reserve(texts.size());
        for (const auto& text : texts) {
            if (stop.stop_requested()) {
                throw TranslateError("翻译已取消");
            }
            if (trim(text).empty()) {
                output.emplace_back();
                continue;
            }
            const std::vector<std::wstring> one_input{text};
            const auto raw = call(one_input, false);
            auto one = parse_ai_array(raw, 1);
            output.push_back(one ? std::move((*one)[0]) : plain_ai(raw));
        }
    }
    std::vector<std::size_t> bad;
    for (std::size_t index = 0; index < texts.size(); ++index) {
        if (!trim(texts[index]).empty() && !valid_ai_output(texts[index], output[index], target)) {
            bad.push_back(index);
        }
    }
    if (!bad.empty()) {
        std::vector<std::wstring> retry_input;
        retry_input.reserve(bad.size());
        for (const auto index : bad) retry_input.push_back(texts[index]);
        const auto again = parse_ai_array(call(retry_input, true), retry_input.size());
        if (!again) {
            throw TranslateError("模型没有按要求输出目标语言，重试格式仍不正确");
        }
        for (std::size_t index = 0; index < bad.size(); ++index) {
            if (valid_ai_output(texts[bad[index]], (*again)[index], target)) {
                output[bad[index]] = (*again)[index];
            }
        }
        for (const auto index : bad) {
            if (!valid_ai_output(texts[index], output[index], target)) {
                throw TranslateError("模型连续两次没有输出目标语言，请重试或更换模型");
            }
        }
    }
    return output;
}

std::vector<std::wstring> TranslatorService::list_models(
    std::wstring_view provider, const JsonObject& options, std::stop_token stop) {
    const auto key = trim(json_string(options, L"key"));
    HttpRequest request;
    if (provider == L"anthropic") {
        if (key.empty()) throw TranslateError("尚未填写 Anthropic API Key");
        const auto base = validate_api_base_url(
            json_string(options, L"base_url", L"https://api.anthropic.com"),
            L"Anthropic endpoint");
        request.url = append_query(base + L"/v1/models", {{L"limit", L"100"}});
        request.headers = {{L"x-api-key", key}, {L"anthropic-version", L"2023-06-01"}};
    } else if (provider == L"openai" || provider == L"nvidia") {
        const auto default_base = provider == L"nvidia"
            ? std::wstring_view(L"https://integrate.api.nvidia.com/v1")
            : std::wstring_view();
        auto base = normalize_ai_root(json_string(options, L"base_url", default_base));
        base = validate_api_base_url(base, L"OpenAI compatible endpoint", !key.empty(), true);
        request.url = base + L"/models";
        if (!key.empty()) request.headers.emplace_back(L"Authorization", L"Bearer " + key);
    } else {
        throw TranslateError("这个翻译引擎不提供模型列表");
    }
    const auto response = http_.send(request, stop);
    require_success(response, L"模型列表",
                    key.empty() ? std::span<const std::wstring>()
                                : std::span<const std::wstring>(&key, 1));
    try {
        const auto values = parse_object(response, L"模型列表").GetNamedArray(L"data");
        std::vector<std::wstring> output;
        for (std::uint32_t index = 0; index < values.Size(); ++index) {
            const auto id = values.GetObjectAt(index).GetNamedString(L"id", L"");
            if (!id.empty()) output.emplace_back(id.c_str());
        }
        if (output.empty()) throw TranslateError("接口没有返回任何模型");
        if (provider != L"anthropic") std::sort(output.begin(), output.end());
        return output;
    } catch (const TranslateError&) {
        throw;
    } catch (...) {
        throw TranslateError("模型列表返回格式异常");
    }
}

}  // namespace screentrans
