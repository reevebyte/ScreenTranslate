#include "pipeline.hpp"

#include "cloud_ocr.hpp"
#include "language.hpp"
#include "layout.hpp"
#include "ocr.hpp"
#include "rapidocr_plugin.hpp"

#include <winrt/Windows.Data.Json.h>

#include <algorithm>
#include <iterator>

namespace screentrans {

using winrt::Windows::Data::Json::JsonObject;

namespace {

constexpr std::size_t lane_index(PipelineLane lane) noexcept {
    return static_cast<std::size_t>(lane);
}

}  // namespace

PipelineOptions PipelineOptions::from_config(const ConfigStore& config) {
    PipelineOptions result;
    result.ocr_engine = config.string(L"ocr.engine", L"windows");
    result.ocr_languages = config.strings(L"ocr.languages");
    result.ocr_upscale = config.boolean(L"ocr.upscale", true);
    result.azure_options_json = config.object(L"ocr.azure_vision").Stringify().c_str();
    result.translator_provider = config.string(L"translator.provider", L"microsoft");
    result.translator_options_json = config.object(
        L"translator." + result.translator_provider).Stringify().c_str();
    result.chinese_target = config.string(L"lang.zh_target", L"en");
    return result;
}

PipelineController::PipelineController(HWND dispatcher)
    : PipelineController(dispatcher, true) {}

PipelineController::PipelineController(HWND dispatcher, bool start_workers)
    : dispatcher_(dispatcher) {
    if (!start_workers) return;
    workers_.reserve(2);
    for (int index = 0; index < 2; ++index) {
        workers_.emplace_back([this](std::stop_token stop) noexcept {
            try {
                WinrtApartment apartment(winrt::apartment_type::multi_threaded);
                worker_loop(stop);
            } catch (...) {
                worker_failed();
            }
        });
    }
}

PipelineController::~PipelineController() {
    {
        std::lock_guard lock(mutex_);
        shutting_down_ = true;
        for (auto& [id, request] : active_) request->cancellation.request_stop();
        for (auto& request : pending_) request->cancellation.request_stop();
        pending_.clear();
    }
    for (auto& worker : workers_) worker.request_stop();
    condition_.notify_all();
    for (auto& worker : workers_) {
        if (worker.joinable()) worker.join();
    }
    workers_.clear();
}

std::uint64_t PipelineController::schedule(std::shared_ptr<const PixelBuffer> image,
                                           PipelineOptions options,
                                           PipelineLane lane) {
    if (!image || image->empty()) {
        throw AppError("cannot schedule an empty screenshot");
    }
    auto request = std::make_shared<Request>();
    {
        std::lock_guard lock(mutex_);
        if (shutting_down_) throw AppError("pipeline is shutting down");
        if (failed_workers_ == 2) throw AppError("pipeline workers are unavailable");
        request->id = ++next_request_;
        request->lane = lane;
        request->image = std::move(image);
        request->options = std::move(options);
        latest_requests_[lane_index(lane)] = request->id;
        for (auto& [id, active] : active_) {
            if (active->lane == lane) active->cancellation.request_stop();
        }
        std::erase_if(pending_, [lane](const auto& queued) {
            if (queued->lane != lane) return false;
            queued->cancellation.request_stop();
            return true;
        });
        pending_.push_back(request);
    }
    condition_.notify_one();
    return request->id;
}

std::uint64_t PipelineController::schedule_translation(std::vector<TextBlock> blocks,
                                                       PipelineOptions options,
                                                       PipelineLane lane) {
    if (blocks.empty()) {
        throw AppError("cannot translate an empty block list");
    }
    auto request = std::make_shared<Request>();
    {
        std::lock_guard lock(mutex_);
        if (shutting_down_) throw AppError("pipeline is shutting down");
        if (failed_workers_ == 2) throw AppError("pipeline workers are unavailable");
        request->id = ++next_request_;
        request->lane = lane;
        request->blocks = std::move(blocks);
        request->options = std::move(options);
        latest_requests_[lane_index(lane)] = request->id;
        for (auto& [id, active] : active_) {
            if (active->lane == lane) active->cancellation.request_stop();
        }
        std::erase_if(pending_, [lane](const auto& queued) {
            if (queued->lane != lane) return false;
            queued->cancellation.request_stop();
            return true;
        });
        pending_.push_back(request);
    }
    condition_.notify_one();
    return request->id;
}

void PipelineController::cancel_lane(PipelineLane lane) {
    std::lock_guard lock(mutex_);
    ++latest_requests_[lane_index(lane)];
    for (auto& [id, request] : active_) {
        if (request->lane == lane) request->cancellation.request_stop();
    }
    std::erase_if(pending_, [lane](const auto& request) {
        if (request->lane != lane) return false;
        request->cancellation.request_stop();
        return true;
    });
    std::erase_if(completions_, [lane](const auto& completion) {
        return completion.lane == lane;
    });
}

void PipelineController::cancel_all() {
    std::lock_guard lock(mutex_);
    for (auto& latest : latest_requests_) ++latest;
    for (auto& [id, request] : active_) request->cancellation.request_stop();
    for (auto& request : pending_) request->cancellation.request_stop();
    pending_.clear();
    completions_.clear();
}

std::vector<PipelineCompletion> PipelineController::take_completions() {
    std::vector<PipelineCompletion> output;
    std::lock_guard lock(mutex_);
    while (!completions_.empty()) {
        output.push_back(std::move(completions_.front()));
        completions_.pop_front();
    }
    return output;
}

std::uint64_t PipelineController::latest_request(PipelineLane lane) const noexcept {
    std::lock_guard lock(mutex_);
    return latest_requests_[lane_index(lane)];
}

void PipelineController::self_test() {
    PipelineController pipeline(nullptr, false);
    PipelineOptions options;
    const auto blocks = [](std::wstring text) {
        TextBlock block;
        OcrLine line;
        line.text = std::move(text);
        line.bounds = RectF{0.0F, 0.0F, 1.0F, 1.0F};
        block.lines.push_back(std::move(line));
        std::vector<TextBlock> result;
        result.push_back(std::move(block));
        return result;
    };

    const auto visual = pipeline.schedule_translation(
        blocks(L"visual"), options, PipelineLane::visual);
    const auto old_text = pipeline.schedule_translation(
        blocks(L"old text"), options, PipelineLane::text);
    const auto current_text = pipeline.schedule_translation(
        blocks(L"current text"), options, PipelineLane::text);
    {
        std::lock_guard lock(pipeline.mutex_);
        if (pipeline.pending_.size() != 2 ||
            pipeline.pending_[0]->id != visual ||
            pipeline.pending_[1]->id != current_text) {
            throw AppError("pipeline lane self-test did not retain one request per lane");
        }
    }
    if (pipeline.latest_request(PipelineLane::visual) != visual ||
        pipeline.latest_request(PipelineLane::text) != current_text) {
        throw AppError("pipeline lane self-test latest request mismatch");
    }

    pipeline.finish(PipelineCompletion{
        old_text, PipelineLane::text, PipelineResult{}, {},
    });
    if (!pipeline.take_completions().empty()) {
        throw AppError("pipeline lane self-test accepted a late completion");
    }
    pipeline.finish(PipelineCompletion{
        visual, PipelineLane::visual, PipelineResult{}, {},
    });
    pipeline.finish(PipelineCompletion{
        current_text, PipelineLane::text, PipelineResult{}, {},
    });
    pipeline.cancel_lane(PipelineLane::visual);
    auto completions = pipeline.take_completions();
    if (completions.size() != 1 || completions[0].lane != PipelineLane::text ||
        completions[0].request_id != current_text) {
        throw AppError("pipeline lane self-test cancellation crossed lanes");
    }

    const auto visual_latest = pipeline.latest_request(PipelineLane::visual);
    const auto text_latest = pipeline.latest_request(PipelineLane::text);
    pipeline.cancel_lane(PipelineLane::text);
    if (pipeline.latest_request(PipelineLane::visual) != visual_latest ||
        pipeline.latest_request(PipelineLane::text) == text_latest) {
        throw AppError("pipeline lane self-test generation cancellation mismatch");
    }
    pipeline.cancel_all();
    {
        std::lock_guard lock(pipeline.mutex_);
        if (!pipeline.pending_.empty() || !pipeline.completions_.empty()) {
            throw AppError("pipeline lane self-test cancel all left queued work");
        }
    }
}

void PipelineController::worker_loop(std::stop_token shutdown) {
    while (!shutdown.stop_requested()) {
        std::shared_ptr<Request> request;
        {
            std::unique_lock lock(mutex_);
            condition_.wait(lock, shutdown, [this] {
                return shutting_down_ || !pending_.empty();
            });
            if (shutdown.stop_requested() || shutting_down_) break;
            request = std::move(pending_.front());
            pending_.pop_front();
            active_[request->id] = request;
        }
        PipelineCompletion completion;
        completion.request_id = request->id;
        completion.lane = request->lane;
        try {
            if (request->cancellation.stop_requested()) {
                throw AppError("request cancelled");
            }
            completion.result = execute(*request, request->cancellation.get_token());
        } catch (const std::exception& error) {
            completion.error = utf8_to_wide(error.what());
        } catch (...) {
            completion.error = L"未知的后台错误";
        }
        {
            std::lock_guard lock(mutex_);
            active_.erase(request->id);
        }
        if (!request->cancellation.stop_requested()) {
            finish(std::move(completion));
        }
        condition_.notify_all();
    }
}

PipelineResult PipelineController::execute(const Request& request, std::stop_token stop) {
    const auto& options = request.options;
    if (!request.blocks.empty()) {
        return translate_blocks(request.blocks, options, stop);
    }
    if (!request.image || request.image->empty()) {
        throw AppError("pipeline request has no screenshot");
    }
    std::vector<OcrLine> lines;
    if (options.ocr_engine == L"windows") {
        lines = recognize_windows_ocr(*request.image, options.ocr_languages,
                                      options.ocr_upscale, stop);
    } else if (options.ocr_engine == L"azure_vision") {
        lines = recognize_azure_vision(cloud_http_, *request.image,
            JsonObject::Parse(options.azure_options_json), stop);
    } else if (options.ocr_engine == L"youdao_cloud") {
        lines = recognize_youdao_cloud(cloud_http_, *request.image, stop);
    } else if (options.ocr_engine == L"rapidocr") {
        lines = recognize_rapidocr_plugin(*request.image, stop);
    } else {
        throw OcrError("未知 OCR 引擎");
    }
    if (stop.stop_requested()) throw AppError("request cancelled");
    auto blocks = group_lines(std::move(lines));
    if (blocks.empty()) throw OcrError("没有识别到文字");

    return translate_blocks(std::move(blocks), options, stop);
}

PipelineResult PipelineController::translate_blocks(std::vector<TextBlock> blocks,
                                                     const PipelineOptions& options,
                                                     std::stop_token stop) {

    struct TargetGroup {
        std::wstring target;
        std::vector<std::size_t> indices;
        std::vector<std::wstring> texts;
    };
    std::vector<TargetGroup> groups;
    for (std::size_t index = 0; index < blocks.size(); ++index) {
        const auto text = blocks[index].text();
        const auto target = blocks[index].forced_target_language.empty()
            ? target_for(text, options.chinese_target)
            : blocks[index].forced_target_language;
        auto found = std::find_if(groups.begin(), groups.end(), [&](const TargetGroup& group) {
            return group.target == target;
        });
        if (found == groups.end()) {
            groups.push_back(TargetGroup{target, {}, {}});
            found = std::prev(groups.end());
        }
        found->indices.push_back(index);
        found->texts.push_back(text);
    }
    std::vector<std::wstring> translated(blocks.size());
    const auto translator_options = JsonObject::Parse(options.translator_options_json);
    for (const auto& group : groups) {
        if (stop.stop_requested()) throw AppError("request cancelled");
        auto values = translators_.translate(options.translator_provider, translator_options,
                                              group.texts, group.target, std::nullopt, stop);
        if (values.size() != group.indices.size()) {
            throw TranslateError("翻译接口返回条数与输入不一致");
        }
        for (std::size_t index = 0; index < values.size(); ++index) {
            translated[group.indices[index]] = std::move(values[index]);
        }
    }

    PipelineResult result;
    result.blocks.reserve(blocks.size());
    for (std::size_t index = 0; index < blocks.size(); ++index) {
        const auto target = blocks[index].forced_target_language.empty()
            ? target_for(blocks[index].text(), options.chinese_target)
            : blocks[index].forced_target_language;
        if (!result.plain_text.empty() && !translated[index].empty()) result.plain_text.push_back(L'\n');
        result.plain_text += translated[index];
        result.blocks.push_back(BlockTranslation{
            std::move(blocks[index]), std::move(translated[index]), target,
        });
    }
    return result;
}

void PipelineController::finish(PipelineCompletion completion) {
    {
        std::lock_guard lock(mutex_);
        if (shutting_down_ ||
            completion.request_id != latest_requests_[lane_index(completion.lane)]) {
            return;
        }
        completions_.push_back(std::move(completion));
    }
    if (dispatcher_) PostMessageW(dispatcher_, pipeline_completed_message, 0, 0);
}

void PipelineController::worker_failed() noexcept {
    bool notify = false;
    try {
        std::lock_guard lock(mutex_);
        ++failed_workers_;
        if (failed_workers_ != 2 || shutting_down_) return;
        std::array<bool, 2> affected{};
        for (const auto& request : pending_) affected[lane_index(request->lane)] = true;
        for (const auto& [id, request] : active_) affected[lane_index(request->lane)] = true;
        for (auto& request : pending_) request->cancellation.request_stop();
        for (auto& [id, request] : active_) request->cancellation.request_stop();
        pending_.clear();
        active_.clear();
        for (std::size_t index = 0; index < affected.size(); ++index) {
            if (!affected[index]) continue;
            completions_.push_back(PipelineCompletion{
                latest_requests_[index], static_cast<PipelineLane>(index), std::nullopt,
                L"后台处理线程初始化失败，请重启 ScreenTranslate",
            });
            notify = true;
        }
    } catch (...) {
        return;
    }
    if (notify) PostMessageW(dispatcher_, pipeline_completed_message, 0, 0);
}

}  // namespace screentrans
