cmake_minimum_required(VERSION 3.25)

project(ScreenTranslateNative VERSION 1.1.0 LANGUAGES CXX RC)

set(SCREENTRANS_UPDATE_REPOSITORY_URL
    "https://github.com/reevebyte/ScreenTranslate"
    CACHE STRING "Immutable GitHub repository trusted by the native updater")
set(SCREENTRANS_UPDATE_MANIFEST_URL
    "https://github.com/reevebyte/ScreenTranslate/releases/latest/download/update-manifest.json"
    CACHE STRING "Immutable GitHub Release manifest URL trusted by the native updater")
set(SCREENTRANS_UPDATE_CHANNEL "stable" CACHE STRING "Native update channel")

if(NOT WIN32)
    message(FATAL_ERROR "The native ScreenTranslate client only supports Windows.")
endif()

if(NOT MSVC)
    message(FATAL_ERROR "The native ScreenTranslate client currently requires MSVC.")
endif()

add_executable(ScreenTranslate WIN32
    src/app.cpp
    src/app.hpp
    src/block_editor.cpp
    src/block_editor.hpp
    src/capture.cpp
    src/capture.hpp
    src/config.cpp
    src/config.hpp
    src/cloud_ocr.cpp
    src/cloud_ocr.hpp
    src/crypto.cpp
    src/crypto.hpp
    src/dpapi.cpp
    src/dpapi.hpp
    src/image_codec.cpp
    src/image_codec.hpp
    src/http.cpp
    src/http.hpp
    src/hotkey.cpp
    src/hotkey.hpp
    src/language.cpp
    src/language.hpp
    src/layout.cpp
    src/layout.hpp
    src/main.cpp
    src/ocr.cpp
    src/ocr.hpp
    src/pipeline.cpp
    src/pipeline.hpp
    src/rapidocr_plugin.cpp
    src/rapidocr_plugin.hpp
    src/resource.h
    src/result_renderer.cpp
    src/result_renderer.hpp
    src/result_window.cpp
    src/result_window.hpp
    src/ScreenTranslate.rc
    src/selection_overlay.cpp
    src/selection_overlay.hpp
    src/settings_window.cpp
    src/settings_window.hpp
    src/text_translation_session.cpp
    src/text_translation_session.hpp
    src/text_translation_window.cpp
    src/text_translation_window.hpp
    src/types.hpp
    src/updater.cpp
    src/updater.hpp
    src/update_window.cpp
    src/update_window.hpp
    src/translator.cpp
    src/translator.hpp
    src/util.cpp
    src/util.hpp
    src/version.hpp
)

target_compile_features(ScreenTranslate PRIVATE cxx_std_20)

set_target_properties(ScreenTranslate PROPERTIES
    OUTPUT_NAME "ScreenTranslate"
    MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>"
    INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE
)

target_compile_definitions(ScreenTranslate PRIVATE
    UNICODE
    _UNICODE
    WIN32_LEAN_AND_MEAN
    NOMINMAX
    STRICT
    WINVER=0x0A00
    _WIN32_WINNT=0x0A00
    "SCREENTRANS_VERSION_WIDE=L\"${PROJECT_VERSION}\""
    "SCREENTRANS_UPDATE_REPOSITORY_URL=L\"${SCREENTRANS_UPDATE_REPOSITORY_URL}\""
    "SCREENTRANS_UPDATE_MANIFEST_URL=L\"${SCREENTRANS_UPDATE_MANIFEST_URL}\""
    "SCREENTRANS_UPDATE_CHANNEL=L\"${SCREENTRANS_UPDATE_CHANNEL}\""
)

target_include_directories(ScreenTranslate PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/src"
    "${CMAKE_CURRENT_SOURCE_DIR}/.."
)

target_compile_options(ScreenTranslate PRIVATE
    /W4
    /permissive-
    /EHsc
    /utf-8
    $<$<CONFIG:Release>:/O2>
)

target_link_options(ScreenTranslate PRIVATE
    $<$<CONFIG:Debug>:/DEBUG:FULL>
    $<$<CONFIG:Release>:/LTCG>
    $<$<CONFIG:Release>:/OPT:REF>
    $<$<CONFIG:Release>:/OPT:ICF>
)

target_link_libraries(ScreenTranslate PRIVATE
    user32
    shell32
    comctl32
    gdi32
    d2d1
    dwrite
    windowscodecs
    winhttp
    crypt32
    bcrypt
    ole32
    runtimeobject
    windowsapp
    shcore
    dwmapi
    msimg32
    uxtheme
)
