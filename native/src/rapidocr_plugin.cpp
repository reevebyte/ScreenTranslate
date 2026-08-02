#include "rapidocr_plugin.hpp"

#include "ocr.hpp"
#include "util.hpp"

#ifdef GetObject
#undef GetObject
#endif

#include <roapi.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Data.Json.h>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <utility>

namespace screentrans {

using namespace winrt::Windows::Data::Json;

namespace {

constexpr std::uint64_t maximum_plugin_json_size = 8ULL * 1024 * 1024;
constexpr std::uint64_t maximum_plugin_error_size = 64ULL * 1024;
constexpr std::uint32_t maximum_line_count = 16'384;
constexpr std::uint32_t maximum_word_count = 65'536;
constexpr std::size_t maximum_text_length = 256 * 1024;
constexpr double bounds_epsilon = 1.0;

class ModuleHandle {
public:
    explicit ModuleHandle(HMODULE value) noexcept : value_(value) {}
    ~ModuleHandle() {
        if (value_) FreeLibrary(value_);
    }

    ModuleHandle(const ModuleHandle&) = delete;
    ModuleHandle& operator=(const ModuleHandle&) = delete;
    ModuleHandle(ModuleHandle&&) = delete;
    ModuleHandle& operator=(ModuleHandle&&) = delete;

    [[nodiscard]] HMODULE get() const noexcept { return value_; }

private:
    HMODULE value_{};
};

class RuntimeApartmentScope {
public:
    RuntimeApartmentScope() {
        result_ = RoInitialize(RO_INIT_MULTITHREADED);
        if (FAILED(result_) && result_ != RPC_E_CHANGED_MODE) {
            throw OcrError("RapidOCR self-test could not initialize Windows Runtime");
        }
    }

    ~RuntimeApartmentScope() {
        if (SUCCEEDED(result_)) RoUninitialize();
    }

private:
    HRESULT result_{};
};

struct PluginApi {
    explicit PluginApi(HMODULE value) : module(value) {}

    ModuleHandle module;
    ScreenTranslateRapidOcrGetAbiVersionFn get_version{};
    ScreenTranslateRapidOcrRecognizeFn recognize{};
    ScreenTranslateRapidOcrReleaseResultFn release{};
};

struct PluginState {
    std::mutex mutex;
    std::unique_ptr<PluginApi> api;
};

PluginState& plugin_state() {
    static PluginState value;
    return value;
}

std::wstring final_path(HANDLE handle) {
    const DWORD needed = GetFinalPathNameByHandleW(
        handle, nullptr, 0, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
    if (needed == 0) throw_last_error("resolve RapidOCR path");
    std::wstring output(static_cast<std::size_t>(needed), L'\0');
    const DWORD written = GetFinalPathNameByHandleW(
        handle, output.data(), needed, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
    if (written == 0 || written >= needed) {
        throw_last_error("resolve RapidOCR path");
    }
    output.resize(written);
    while (output.size() > 1 &&
           (output.back() == L'\\' || output.back() == L'/')) {
        output.pop_back();
    }
    return output;
}

bool same_path(std::wstring_view left, std::wstring_view right) noexcept {
    if (left.size() > static_cast<std::size_t>(std::numeric_limits<int>::max()) ||
        right.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        return false;
    }
    return CompareStringOrdinal(
               left.data(), static_cast<int>(left.size()),
               right.data(), static_cast<int>(right.size()), TRUE) == CSTR_EQUAL;
}

void require_direct_child(const std::filesystem::path& directory,
                          const std::filesystem::path& child) {
    const auto normalized_directory = directory.lexically_normal();
    const auto normalized_child = child.lexically_normal();
    if (!same_path(normalized_child.parent_path().native(),
                   normalized_directory.native())) {
        throw OcrError("RapidOCR path escaped the ScreenTranslate directory");
    }
}

UniqueHandle open_application_directory(const std::filesystem::path& directory) {
    UniqueHandle handle(CreateFileW(
        directory.c_str(), FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
        OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr));
    if (!handle) throw_last_error("open ScreenTranslate directory");
    return handle;
}

void require_not_reparse(HANDLE handle, std::string_view label, bool expect_directory) {
    FILE_ATTRIBUTE_TAG_INFO attributes{};
    if (!GetFileInformationByHandleEx(
            handle, FileAttributeTagInfo, &attributes, sizeof(attributes))) {
        throw_last_error(std::string("inspect ") + std::string(label));
    }
    const bool directory = (attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    if (directory != expect_directory) {
        throw OcrError(std::string(label) +
                       (expect_directory ? " is not a directory" : " is not a regular file"));
    }
    if ((attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
        throw OcrError(std::string(label) + " must not be a reparse point");
    }
}

void require_same_resolved_parent(HANDLE file,
                                  const std::wstring& application_directory) {
    const auto resolved = std::filesystem::path(final_path(file));
    if (!same_path(resolved.parent_path().native(), application_directory)) {
        throw OcrError("RapidOCR DLL resolved outside the ScreenTranslate directory");
    }
}

void read_exact(HANDLE file, std::uint64_t offset, void* output, DWORD size) {
    LARGE_INTEGER location{};
    location.QuadPart = static_cast<LONGLONG>(offset);
    if (!SetFilePointerEx(file, location, nullptr, FILE_BEGIN)) {
        throw_last_error("inspect RapidOCR DLL");
    }
    DWORD read = 0;
    if (!ReadFile(file, output, size, &read, nullptr) || read != size) {
        throw OcrError("RapidOCR DLL has a truncated PE header");
    }
}

void validate_x64_dll(HANDLE file) {
    LARGE_INTEGER file_size{};
    if (!GetFileSizeEx(file, &file_size) || file_size.QuadPart <= 0) {
        throw OcrError("RapidOCR DLL has an invalid file size");
    }

    IMAGE_DOS_HEADER dos{};
    read_exact(file, 0, &dos, sizeof(dos));
    if (dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew < sizeof(dos)) {
        throw OcrError("RapidOCR plug-in is not a valid Windows DLL");
    }
    const auto header_offset = static_cast<std::uint64_t>(dos.e_lfanew);
    constexpr std::uint64_t minimum_nt_size =
        sizeof(DWORD) + sizeof(IMAGE_FILE_HEADER) + sizeof(WORD);
    if (header_offset > static_cast<std::uint64_t>(file_size.QuadPart) ||
        static_cast<std::uint64_t>(file_size.QuadPart) - header_offset < minimum_nt_size) {
        throw OcrError("RapidOCR DLL has an invalid PE header offset");
    }

    DWORD signature = 0;
    IMAGE_FILE_HEADER header{};
    WORD optional_magic = 0;
    read_exact(file, header_offset, &signature, sizeof(signature));
    read_exact(file, header_offset + sizeof(signature), &header, sizeof(header));
    read_exact(file, header_offset + sizeof(signature) + sizeof(header),
               &optional_magic, sizeof(optional_magic));
    if (signature != IMAGE_NT_SIGNATURE || header.NumberOfSections == 0 ||
        (header.Characteristics & IMAGE_FILE_DLL) == 0) {
        throw OcrError("RapidOCR plug-in is not a valid Windows DLL");
    }
    if (header.Machine != IMAGE_FILE_MACHINE_AMD64 ||
        optional_magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC) {
        throw OcrError("RapidOCR plug-in must be an x64 DLL");
    }
}

UniqueHandle validate_plugin_file(const std::filesystem::path& path,
                                  const std::filesystem::path& directory,
                                  const std::wstring& resolved_directory) {
    require_direct_child(directory, path);
    UniqueHandle file(CreateFileW(
        path.c_str(), GENERIC_READ | FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        nullptr, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
    if (!file) {
        const DWORD error = GetLastError();
        if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) {
            throw OcrError(
                "RapidOCR plug-in is not installed. Put " +
                wide_to_utf8(std::filesystem::path(rapidocr_plugin_filename).wstring()) +
                " beside ScreenTranslate.exe.");
        }
        throw_last_error("open RapidOCR plug-in", error);
    }
    require_not_reparse(file.get(), "RapidOCR DLL", false);
    require_same_resolved_parent(file.get(), resolved_directory);
    validate_x64_dll(file.get());
    return file;
}

UniqueHandle validate_model_directory(const std::filesystem::path& path,
                                      const std::filesystem::path& directory,
                                      const std::wstring& resolved_directory) {
    require_direct_child(directory, path);
    UniqueHandle model_directory(CreateFileW(
        path.c_str(), FILE_READ_ATTRIBUTES, FILE_SHARE_READ, nullptr,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
    if (!model_directory) {
        const DWORD error = GetLastError();
        if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) {
            throw OcrError(
                "RapidOCR models are not installed. Put the model files in " +
                wide_to_utf8(std::filesystem::path(rapidocr_model_directory_name).wstring()) +
                " beside ScreenTranslate.exe.");
        }
        throw_last_error("open RapidOCR model directory", error);
    }
    require_not_reparse(model_directory.get(), "RapidOCR model directory", true);
    const auto resolved = std::filesystem::path(final_path(model_directory.get()));
    if (!same_path(resolved.parent_path().native(), resolved_directory)) {
        throw OcrError("RapidOCR model directory resolved outside ScreenTranslate");
    }
    return model_directory;
}

template <typename Function>
Function required_export(HMODULE module, const char* name) {
    const FARPROC raw = GetProcAddress(module, name);
    if (!raw) {
        throw OcrError(std::string("RapidOCR plug-in is missing export ") + name);
    }
    static_assert(sizeof(Function) == sizeof(raw));
    Function output{};
    std::memcpy(&output, &raw, sizeof(output));
    return output;
}

std::unique_ptr<PluginApi> load_plugin(
    const std::filesystem::path& path,
    const std::filesystem::path& directory,
    const std::wstring& resolved_directory) {
    // Keep the validated file handle open until LoadLibraryEx has mapped it.
    auto file = validate_plugin_file(path, directory, resolved_directory);
    HMODULE module = LoadLibraryExW(
        path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        const DWORD error = GetLastError();
        throw OcrError(
            "RapidOCR plug-in could not be loaded (Windows error " +
            std::to_string(error) +
            "). A native dependency may be missing or incompatible.");
    }

    auto api = std::make_unique<PluginApi>(module);
    api->get_version = required_export<ScreenTranslateRapidOcrGetAbiVersionFn>(
        module, "ScreenTranslateRapidOcrGetAbiVersion");
    api->recognize = required_export<ScreenTranslateRapidOcrRecognizeFn>(
        module, "ScreenTranslateRapidOcrRecognize");
    api->release = required_export<ScreenTranslateRapidOcrReleaseResultFn>(
        module, "ScreenTranslateRapidOcrReleaseResult");
    if (api->get_version() != SCREENTRANS_RAPIDOCR_ABI_VERSION) {
        throw OcrError(
            "RapidOCR plug-in ABI is incompatible with this ScreenTranslate version");
    }
    return api;
}

std::wstring json_text(const JsonObject& object, std::wstring_view name) {
    if (!object.HasKey(name)) {
        throw OcrError("RapidOCR JSON is missing required text");
    }
    const auto value = object.Lookup(name);
    if (value.ValueType() != JsonValueType::String) {
        throw OcrError("RapidOCR JSON text must be a string");
    }
    std::wstring text = value.GetString().c_str();
    if (text.size() > maximum_text_length ||
        std::find(text.begin(), text.end(), L'\0') != text.end()) {
        throw OcrError("RapidOCR JSON contains invalid text");
    }
    for (std::size_t index = 0; index < text.size(); ++index) {
        const auto value16 = static_cast<std::uint16_t>(text[index]);
        if (value16 >= 0xD800 && value16 <= 0xDBFF) {
            if (++index >= text.size()) {
                throw OcrError("RapidOCR JSON contains invalid Unicode");
            }
            const auto low = static_cast<std::uint16_t>(text[index]);
            if (low < 0xDC00 || low > 0xDFFF) {
                throw OcrError("RapidOCR JSON contains invalid Unicode");
            }
        } else if (value16 >= 0xDC00 && value16 <= 0xDFFF) {
            throw OcrError("RapidOCR JSON contains invalid Unicode");
        }
    }
    return text;
}

float json_confidence(const JsonObject& object) {
    if (!object.HasKey(L"confidence")) return -1.0F;
    const auto value = object.Lookup(L"confidence");
    if (value.ValueType() != JsonValueType::Number) {
        throw OcrError("RapidOCR confidence must be a number");
    }
    const double confidence = value.GetNumber();
    if (!std::isfinite(confidence) || confidence < 0.0 || confidence > 1.0) {
        throw OcrError("RapidOCR confidence must be between 0 and 1");
    }
    return static_cast<float>(confidence);
}

RectF json_bounds(const JsonObject& object, float image_width, float image_height) {
    if (!object.HasKey(L"bounds") ||
        object.Lookup(L"bounds").ValueType() != JsonValueType::Array) {
        throw OcrError("RapidOCR JSON is missing bounds");
    }
    const auto values = object.Lookup(L"bounds").GetArray();
    if (values.Size() != 4) {
        throw OcrError("RapidOCR bounds must contain four numbers");
    }
    double numbers[4]{};
    for (std::uint32_t index = 0; index < 4; ++index) {
        const auto value = values.GetAt(index);
        if (value.ValueType() != JsonValueType::Number) {
            throw OcrError("RapidOCR bounds must contain only numbers");
        }
        numbers[index] = value.GetNumber();
        if (!std::isfinite(numbers[index])) {
            throw OcrError("RapidOCR bounds must be finite");
        }
    }
    const double right = numbers[0] + numbers[2];
    const double bottom = numbers[1] + numbers[3];
    if (numbers[2] <= 0.0 || numbers[3] <= 0.0 ||
        numbers[0] < -bounds_epsilon || numbers[1] < -bounds_epsilon ||
        right > static_cast<double>(image_width) + bounds_epsilon ||
        bottom > static_cast<double>(image_height) + bounds_epsilon) {
        throw OcrError("RapidOCR bounds are outside the source image");
    }

    const float left = static_cast<float>(std::clamp(numbers[0], 0.0,
                                                      static_cast<double>(image_width)));
    const float top = static_cast<float>(std::clamp(numbers[1], 0.0,
                                                    static_cast<double>(image_height)));
    const float clamped_right = static_cast<float>(std::clamp(
        right, 0.0, static_cast<double>(image_width)));
    const float clamped_bottom = static_cast<float>(std::clamp(
        bottom, 0.0, static_cast<double>(image_height)));
    RectF output{left, top, clamped_right - left, clamped_bottom - top};
    if (output.empty()) throw OcrError("RapidOCR bounds became empty after validation");
    return output;
}

float weighted_confidence(const std::vector<OcrWord>& words) {
    double score = 0.0;
    double weight = 0.0;
    for (const auto& word : words) {
        if (word.confidence < 0.0F) continue;
        const double characters = static_cast<double>(
            std::max<std::size_t>(1, word.text.size()));
        score += word.confidence * characters;
        weight += characters;
    }
    return weight == 0.0 ? -1.0F : static_cast<float>(score / weight);
}

std::vector<OcrLine> parse_result_json(std::string_view json,
                                       int image_width,
                                       int image_height) {
    if (image_width <= 0 || image_height <= 0 || json.empty() ||
        json.size() > maximum_plugin_json_size ||
        std::find(json.begin(), json.end(), '\0') != json.end()) {
        throw OcrError("RapidOCR plug-in returned invalid JSON metadata");
    }
    try {
        const auto root = JsonObject::Parse(utf8_to_wide(json));
        if (!root.HasKey(L"schema_version") ||
            root.Lookup(L"schema_version").ValueType() != JsonValueType::Number ||
            root.Lookup(L"schema_version").GetNumber() != 1.0) {
            throw OcrError("RapidOCR JSON schema version is missing or unsupported");
        }
        if (!root.HasKey(L"lines") ||
            root.Lookup(L"lines").ValueType() != JsonValueType::Array) {
            throw OcrError("RapidOCR JSON is missing the lines array");
        }
        const auto source_lines = root.Lookup(L"lines").GetArray();
        if (source_lines.Size() > maximum_line_count) {
            throw OcrError("RapidOCR plug-in returned too many lines");
        }

        std::vector<OcrLine> output;
        output.reserve(source_lines.Size());
        std::uint32_t word_count = 0;
        for (std::uint32_t line_index = 0;
             line_index < source_lines.Size(); ++line_index) {
            const auto source_value = source_lines.GetAt(line_index);
            if (source_value.ValueType() != JsonValueType::Object) {
                throw OcrError("RapidOCR lines must be JSON objects");
            }
            const auto source = source_value.GetObject();
            OcrLine line;
            line.text = json_text(source, L"text");
            line.bounds = json_bounds(source, static_cast<float>(image_width),
                                      static_cast<float>(image_height));
            line.confidence = json_confidence(source);

            if (source.HasKey(L"words")) {
                const auto words_value = source.Lookup(L"words");
                if (words_value.ValueType() != JsonValueType::Array) {
                    throw OcrError("RapidOCR words must be an array");
                }
                const auto words = words_value.GetArray();
                if (words.Size() > maximum_word_count - word_count) {
                    throw OcrError("RapidOCR plug-in returned too many words");
                }
                word_count += words.Size();
                line.words.reserve(words.Size());
                for (std::uint32_t word_index = 0;
                     word_index < words.Size(); ++word_index) {
                    const auto word_value = words.GetAt(word_index);
                    if (word_value.ValueType() != JsonValueType::Object) {
                        throw OcrError("RapidOCR words must be JSON objects");
                    }
                    const auto word_source = word_value.GetObject();
                    OcrWord word;
                    word.text = json_text(word_source, L"text");
                    word.bounds = json_bounds(
                        word_source, static_cast<float>(image_width),
                        static_cast<float>(image_height));
                    word.confidence = json_confidence(word_source);
                    if (!trim(word.text).empty()) {
                        line.words.push_back(std::move(word));
                    }
                }
            }
            if (trim(line.text).empty()) continue;
            if (line.words.empty()) {
                line.words.push_back(OcrWord{line.text, line.bounds, line.confidence});
            } else if (line.confidence < 0.0F) {
                line.confidence = weighted_confidence(line.words);
            }
            output.push_back(std::move(line));
        }
        return output;
    } catch (const OcrError&) {
        throw;
    } catch (const std::exception&) {
        throw OcrError("RapidOCR plug-in returned malformed JSON");
    } catch (...) {
        throw OcrError("RapidOCR plug-in returned malformed JSON");
    }
}

int32_t SCREENTRANS_RAPIDOCR_CALL cancellation_probe(void* context) {
    if (!context) return 0;
    return static_cast<const std::stop_token*>(context)->stop_requested() ? 1 : 0;
}

std::string copy_plugin_buffer(const char* bytes, std::uint64_t size,
                               std::uint64_t maximum, std::string_view label) {
    if (size == 0) return {};
    if (!bytes || size > maximum ||
        size > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max())) {
        throw OcrError("RapidOCR plug-in returned an invalid " + std::string(label));
    }
    return std::string(bytes, static_cast<std::size_t>(size));
}

struct ResultReleaser {
    ScreenTranslateRapidOcrReleaseResultFn release{};
    ScreenTranslateRapidOcrResultV1* result{};

    ~ResultReleaser() noexcept {
        if (!release || !result) return;
        try {
            release(result);
        } catch (...) {
            // Plug-ins must not throw across the ABI. Do not turn result cleanup
            // into std::terminate if a broken plug-in violates that contract.
        }
    }
};

void throw_plugin_status(std::int32_t status, const std::string& error) {
    if (status == SCREENTRANS_RAPIDOCR_STATUS_CANCELLED) {
        throw OcrError("OCR cancelled");
    }
    std::string detail;
    if (!error.empty()) {
        try {
            const auto wide = utf8_to_wide(error);
            if (std::find(wide.begin(), wide.end(), L'\0') != wide.end()) {
                throw AppError("embedded NUL");
            }
            detail = wide_to_utf8(trim(wide));
        } catch (...) {
            throw OcrError("RapidOCR plug-in returned invalid UTF-8 error text");
        }
    }
    std::string message = "RapidOCR plug-in failed (status " +
        std::to_string(status) + ")";
    if (!detail.empty()) message += ": " + detail;
    throw OcrError(message);
}

}  // namespace

std::filesystem::path rapidocr_plugin_path() {
    return executable_path().parent_path() / rapidocr_plugin_filename;
}

std::filesystem::path rapidocr_model_directory() {
    return executable_path().parent_path() / rapidocr_model_directory_name;
}

std::vector<OcrLine> recognize_rapidocr_plugin(
    const PixelBuffer& image,
    std::stop_token stop) {
    RuntimeApartmentScope apartment;
    if (image.width <= 0 || image.height <= 0 || image.stride <= 0 ||
        image.bgra.empty() ||
        static_cast<std::uint64_t>(image.stride) <
            static_cast<std::uint64_t>(image.width) * 4) {
        throw OcrError("RapidOCR image is empty or invalid");
    }
    const std::uint64_t pixel_size =
        static_cast<std::uint64_t>(image.stride) *
        static_cast<std::uint64_t>(image.height);
    if (pixel_size > image.bgra.size()) {
        throw OcrError("RapidOCR image buffer is truncated");
    }
    if (stop.stop_requested()) throw OcrError("OCR cancelled");

    const auto directory = executable_path().parent_path();
    const auto plugin_path = directory / rapidocr_plugin_filename;
    const auto models_path = directory / rapidocr_model_directory_name;
    auto application_directory = open_application_directory(directory);
    const auto resolved_directory = final_path(application_directory.get());

    std::string json;
    std::string plugin_error;
    std::int32_t status = SCREENTRANS_RAPIDOCR_STATUS_INTERNAL_ERROR;
    auto& state = plugin_state();
    {
        // RapidOCR runtimes commonly reuse mutable inference buffers. The host
        // serializes calls so a minimal plug-in does not need its own lock.
        std::lock_guard lock(state.mutex);
        if (stop.stop_requested()) throw OcrError("OCR cancelled");
        if (!state.api) {
            state.api = load_plugin(plugin_path, directory, resolved_directory);
        }
        auto model_directory = validate_model_directory(
            models_path, directory, resolved_directory);
        const auto models_utf8 = wide_to_utf8(models_path.wstring());

        ScreenTranslateRapidOcrImageV1 plugin_image{};
        plugin_image.struct_size = sizeof(plugin_image);
        plugin_image.width = static_cast<std::uint32_t>(image.width);
        plugin_image.height = static_cast<std::uint32_t>(image.height);
        plugin_image.stride = static_cast<std::uint32_t>(image.stride);
        plugin_image.pixel_format = SCREENTRANS_RAPIDOCR_PIXEL_FORMAT_BGRA8;
        plugin_image.pixels = image.bgra.data();
        plugin_image.pixel_size = pixel_size;

        ScreenTranslateRapidOcrRequestV1 request{};
        request.struct_size = sizeof(request);
        request.model_directory_utf8 = models_utf8.data();
        request.model_directory_size = models_utf8.size();
        request.is_cancelled = cancellation_probe;
        request.cancellation_context = &stop;

        if (stop.stop_requested()) throw OcrError("OCR cancelled");
        ScreenTranslateRapidOcrResultV1 result{};
        result.struct_size = sizeof(result);
        ResultReleaser release{state.api->release, &result};
        status = state.api->recognize(&plugin_image, &request, &result);
        if (result.struct_size < sizeof(ScreenTranslateRapidOcrResultV1)) {
            throw OcrError("RapidOCR plug-in returned an incompatible result structure");
        }
        if (status == SCREENTRANS_RAPIDOCR_STATUS_OK) {
            json = copy_plugin_buffer(result.json_utf8, result.json_size,
                                      maximum_plugin_json_size, "JSON buffer");
            if (json.empty()) {
                throw OcrError("RapidOCR plug-in returned an empty JSON result");
            }
        } else {
            plugin_error = copy_plugin_buffer(
                result.error_utf8, result.error_size,
                maximum_plugin_error_size, "error buffer");
        }
    }

    if (stop.stop_requested()) throw OcrError("OCR cancelled");
    if (status != SCREENTRANS_RAPIDOCR_STATUS_OK) {
        throw_plugin_status(status, plugin_error);
    }
    return parse_result_json(json, image.width, image.height);
}

void rapidocr_plugin_self_test() {
    RuntimeApartmentScope apartment;
    const std::string valid =
        R"({"schema_version":1,"lines":[{"text":"Hello world","bounds":[1,2,30,10],"confidence":0.9,"words":[{"text":"Hello","bounds":[1,2,12,10],"confidence":0.8},{"text":"world","bounds":[15,2,16,10],"confidence":1.0}]}]})";
    const auto parsed = parse_result_json(valid, 100, 50);
    if (parsed.size() != 1 || parsed.front().words.size() != 2 ||
        parsed.front().text != L"Hello world") {
        throw OcrError("RapidOCR JSON parser self-test failed");
    }

    bool rejected_bounds = false;
    try {
        (void)parse_result_json(
            R"({"schema_version":1,"lines":[{"text":"bad","bounds":[0,0,102,10]}]})",
            100, 50);
    } catch (const OcrError&) {
        rejected_bounds = true;
    }
    if (!rejected_bounds) {
        throw OcrError("RapidOCR bounds validation self-test failed");
    }

    const auto directory = executable_path().parent_path();
    bool rejected_escape = false;
    try {
        require_direct_child(
            directory, directory.parent_path() / rapidocr_plugin_filename);
    } catch (const OcrError&) {
        rejected_escape = true;
    }
    if (!rejected_escape) {
        throw OcrError("RapidOCR path containment self-test failed");
    }

    auto application_directory = open_application_directory(directory);
    const auto resolved_directory = final_path(application_directory.get());
    bool reported_missing = false;
    try {
        (void)validate_plugin_file(
            directory / L"ScreenTranslate.RapidOcr.self-test-missing.dll",
            directory, resolved_directory);
    } catch (const OcrError& error) {
        reported_missing = std::string_view(error.what()).find("not installed") !=
            std::string_view::npos;
    }
    if (!reported_missing) {
        throw OcrError("RapidOCR missing plug-in diagnostic self-test failed");
    }
}

}  // namespace screentrans
