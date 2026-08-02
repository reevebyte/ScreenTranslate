#pragma once

// ScreenTranslate RapidOCR plug-in ABI, version 1.
//
// The optional plug-in is loaded dynamically; the main executable never links
// against these exports. A plug-in implementation should define
// SCREENTRANS_RAPIDOCR_PLUGIN_BUILD before including this file, and export the
// three functions declared below. The ABI is x64-only and uses fixed-width
// integers, UTF-8 strings, and plug-in-owned output buffers so allocations never
// cross CRT boundaries. No function may let a C++ exception cross this ABI.
//
// ScreenTranslateRapidOcrRecognize returns JSON with this schema:
// {
//   "schema_version": 1,
//   "lines": [
//     {
//       "text": "recognized text",
//       "bounds": [x, y, width, height],
//       "confidence": 0.98,
//       "words": [
//         {
//           "text": "recognized",
//           "bounds": [x, y, width, height],
//           "confidence": 0.99
//         }
//       ]
//     }
//   ]
// }
//
// Coordinates are image pixels. Bounds must be finite, have positive size, and
// remain inside the supplied image. Confidence is optional and, when present,
// must be in [0, 1]. "words" is optional; the host creates one word from the
// line when it is absent or empty. The plug-in must keep result buffers valid
// until ScreenTranslateRapidOcrReleaseResult returns.

#include <stdint.h>

#if defined(_MSC_VER)
#define SCREENTRANS_RAPIDOCR_CALL __cdecl
#else
#define SCREENTRANS_RAPIDOCR_CALL
#endif

#if defined(_WIN32) && defined(SCREENTRANS_RAPIDOCR_PLUGIN_BUILD)
#define SCREENTRANS_RAPIDOCR_API __declspec(dllexport)
#else
#define SCREENTRANS_RAPIDOCR_API
#endif

#define SCREENTRANS_RAPIDOCR_ABI_VERSION 1u
#define SCREENTRANS_RAPIDOCR_PIXEL_FORMAT_BGRA8 1u

#define SCREENTRANS_RAPIDOCR_STATUS_OK 0
#define SCREENTRANS_RAPIDOCR_STATUS_CANCELLED 1
#define SCREENTRANS_RAPIDOCR_STATUS_INVALID_ARGUMENT 2
#define SCREENTRANS_RAPIDOCR_STATUS_MODEL_ERROR 3
#define SCREENTRANS_RAPIDOCR_STATUS_ENGINE_ERROR 4
#define SCREENTRANS_RAPIDOCR_STATUS_INTERNAL_ERROR 5

#ifdef __cplusplus
extern "C" {
#endif

#pragma pack(push, 8)

typedef struct ScreenTranslateRapidOcrImageV1 {
    uint32_t struct_size;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t pixel_format;
    uint32_t reserved0;
    const uint8_t* pixels;
    uint64_t pixel_size;
} ScreenTranslateRapidOcrImageV1;

typedef int32_t (SCREENTRANS_RAPIDOCR_CALL *
    ScreenTranslateRapidOcrCancellationFn)(void* context);

typedef struct ScreenTranslateRapidOcrRequestV1 {
    uint32_t struct_size;
    uint32_t flags;
    const char* model_directory_utf8;
    uint64_t model_directory_size;
    ScreenTranslateRapidOcrCancellationFn is_cancelled;
    void* cancellation_context;
    uint64_t reserved0;
} ScreenTranslateRapidOcrRequestV1;

typedef struct ScreenTranslateRapidOcrResultV1 {
    uint32_t struct_size;
    uint32_t reserved0;
    const char* json_utf8;
    uint64_t json_size;
    const char* error_utf8;
    uint64_t error_size;
    uint64_t reserved1;
} ScreenTranslateRapidOcrResultV1;

#pragma pack(pop)

SCREENTRANS_RAPIDOCR_API uint32_t SCREENTRANS_RAPIDOCR_CALL
ScreenTranslateRapidOcrGetAbiVersion(void);

SCREENTRANS_RAPIDOCR_API int32_t SCREENTRANS_RAPIDOCR_CALL
ScreenTranslateRapidOcrRecognize(
    const ScreenTranslateRapidOcrImageV1* image,
    const ScreenTranslateRapidOcrRequestV1* request,
    ScreenTranslateRapidOcrResultV1* result);

SCREENTRANS_RAPIDOCR_API void SCREENTRANS_RAPIDOCR_CALL
ScreenTranslateRapidOcrReleaseResult(ScreenTranslateRapidOcrResultV1* result);

typedef uint32_t (SCREENTRANS_RAPIDOCR_CALL *
    ScreenTranslateRapidOcrGetAbiVersionFn)(void);
typedef int32_t (SCREENTRANS_RAPIDOCR_CALL *
    ScreenTranslateRapidOcrRecognizeFn)(
        const ScreenTranslateRapidOcrImageV1*,
        const ScreenTranslateRapidOcrRequestV1*,
        ScreenTranslateRapidOcrResultV1*);
typedef void (SCREENTRANS_RAPIDOCR_CALL *
    ScreenTranslateRapidOcrReleaseResultFn)(ScreenTranslateRapidOcrResultV1*);

#ifdef __cplusplus
}  // extern "C"

#include "types.hpp"

#include <filesystem>
#include <stop_token>
#include <vector>

namespace screentrans {

inline constexpr wchar_t rapidocr_plugin_filename[] =
    L"ScreenTranslate.RapidOcr.dll";
inline constexpr wchar_t rapidocr_model_directory_name[] =
    L"rapidocr-models";

// The DLL must be a regular x64 PE file directly beside ScreenTranslate.exe.
// The model directory must be a non-reparse directory beside the executable.
// Plug-in dependencies are resolved only from the DLL directory and System32.
std::filesystem::path rapidocr_plugin_path();
std::filesystem::path rapidocr_model_directory();

std::vector<OcrLine> recognize_rapidocr_plugin(
    const PixelBuffer& image,
    std::stop_token stop = {});

// Parser, bounds, path-containment, and missing-DLL checks. This does not load
// or require a real RapidOCR plug-in and is intended for --self-test.
void rapidocr_plugin_self_test();

}  // namespace screentrans

static_assert(sizeof(void*) == 8,
              "The RapidOCR plug-in ABI currently supports x64 only.");
static_assert(sizeof(ScreenTranslateRapidOcrImageV1) == 40);
static_assert(sizeof(ScreenTranslateRapidOcrRequestV1) == 48);
static_assert(sizeof(ScreenTranslateRapidOcrResultV1) == 48);

#endif  // __cplusplus
