#pragma once

#include "config.hpp"
#include "translator.hpp"
#include "types.hpp"

#include <windows.h>

#include <condition_variable>
#include <cstdint>
#include <deque>
#include <memory>
#include <mutex>
#include <optional>
#include <stop_token>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

namespace screentrans {

inline constexpr UINT pipeline_completed_message = WM_APP + 40;

struct PipelineOptions {
    std::wstring ocr_engine;
    std::vector<std::wstring> ocr_languages;
    bool ocr_upscale{true};
    std::wstring azure_options_json;
    std::wstring translator_provider;
    std::wstring translator_options_json;
    std::wstring chinese_target{L"en"};

    static PipelineOptions from_config(const ConfigStore& config);
};

struct PipelineResult {
    std::vector<BlockTranslation> blocks;
    std::wstring plain_text;
};

struct PipelineCompletion {
    std::uint64_t request_id{};
    std::optional<PipelineResult> result;
    std::wstring error;
};

class PipelineController {
public:
    explicit PipelineController(HWND dispatcher);
    ~PipelineController();

    PipelineController(const PipelineController&) = delete;
    PipelineController& operator=(const PipelineController&) = delete;

    std::uint64_t schedule(std::shared_ptr<const PixelBuffer> image,
                           PipelineOptions options);
    std::uint64_t schedule_translation(std::vector<TextBlock> blocks,
                                       PipelineOptions options);
    void cancel_all();
    std::vector<PipelineCompletion> take_completions();
    [[nodiscard]] std::uint64_t latest_request() const noexcept;

private:
    struct Request {
        std::uint64_t id{};
        std::shared_ptr<const PixelBuffer> image;
        std::vector<TextBlock> blocks;
        PipelineOptions options;
        std::stop_source cancellation;
    };

    void worker_loop(std::stop_token shutdown);
    PipelineResult execute(const Request& request, std::stop_token stop);
    PipelineResult translate_blocks(std::vector<TextBlock> blocks,
                                    const PipelineOptions& options,
                                    std::stop_token stop);
    void finish(PipelineCompletion completion);
    void worker_failed() noexcept;

    HWND dispatcher_{};
    mutable std::mutex mutex_;
    std::condition_variable_any condition_;
    std::deque<std::shared_ptr<Request>> pending_;
    std::unordered_map<std::uint64_t, std::shared_ptr<Request>> active_;
    std::deque<PipelineCompletion> completions_;
    std::uint64_t next_request_{};
    std::uint64_t latest_request_{};
    int failed_workers_{};
    bool shutting_down_{};
    std::vector<std::jthread> workers_;
    TranslatorService translators_;
    HttpClient cloud_http_;
};

}  // namespace screentrans
