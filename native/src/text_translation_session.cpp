#include "text_translation_session.hpp"

#include "language.hpp"
#include "util.hpp"

#include <algorithm>
#include <cwctype>

namespace screentrans {

namespace {

constexpr std::array<std::wstring_view, 9> targets{
    L"zh-Hans", L"zh-Hant", L"en", L"ja", L"ko",
    L"fr", L"de", L"es", L"ru",
};

bool supported_target(std::wstring_view value) noexcept {
    return value.empty() || std::find(targets.begin(), targets.end(), value) != targets.end();
}

}  // namespace

const std::array<std::wstring_view, 9>& text_translation_targets() noexcept {
    return targets;
}

TextTranslationSession::TextTranslationSession(ConfigStore& config,
                                               PipelineController& pipeline)
    : config_(config), pipeline_(pipeline) {}

std::size_t TextTranslationSession::character_count() const noexcept {
    std::size_t count = 0;
    for (std::size_t index = 0; index < input_.size(); ++index, ++count) {
        const wchar_t value = input_[index];
        if (value >= 0xD800 && value <= 0xDBFF && index + 1 < input_.size() &&
            input_[index + 1] >= 0xDC00 && input_[index + 1] <= 0xDFFF) {
            ++index;
        }
    }
    return count;
}

bool TextTranslationSession::has_visible_text() const noexcept {
    return std::any_of(input_.begin(), input_.end(), [](wchar_t value) {
        return std::iswspace(value) == 0;
    });
}

bool TextTranslationSession::can_submit() const noexcept {
    return has_visible_text() && character_count() <= max_characters;
}

std::wstring TextTranslationSession::effective_target() const {
    if (!target_.empty()) return target_;
    return target_for(input_, config_.string(L"lang.zh_target", L"en"));
}

std::wstring TextTranslationSession::configuration_signature() const {
    const auto provider = config_.string(L"translator.provider", L"microsoft");
    return provider + L"\n" +
           std::wstring(config_.object(L"translator." + provider).Stringify().c_str()) +
           L"\n" + config_.string(L"lang.zh_target", L"en");
}

void TextTranslationSession::notify_changed() {
    if (changed_callback_) changed_callback_();
}

void TextTranslationSession::invalidate(TextTranslationState next_state) {
    pipeline_.cancel_lane(PipelineLane::text);
    request_id_ = 0;
    output_.clear();
    error_.clear();
    effective_target_.clear();
    submitted_configuration_.clear();
    state_ = next_state;
}

bool TextTranslationSession::set_input(std::wstring value) {
    if (value == input_) return false;
    input_ = std::move(value);
    if (!has_visible_text()) {
        invalidate(TextTranslationState::idle);
    } else if (character_count() > max_characters) {
        invalidate(TextTranslationState::too_long);
    } else {
        invalidate(TextTranslationState::waiting);
    }
    notify_changed();
    return true;
}

bool TextTranslationSession::set_target(std::wstring value) {
    if (!supported_target(value)) return false;
    if (value == target_) return false;
    target_ = std::move(value);
    if (!has_visible_text()) {
        invalidate(TextTranslationState::idle);
    } else if (character_count() > max_characters) {
        invalidate(TextTranslationState::too_long);
    } else {
        invalidate(TextTranslationState::waiting);
    }
    notify_changed();
    return true;
}

void TextTranslationSession::clear() {
    if (input_.empty() && output_.empty() && state_ == TextTranslationState::idle) return;
    input_.clear();
    invalidate(TextTranslationState::idle);
    notify_changed();
}

bool TextTranslationSession::submit() {
    if (!can_submit()) return false;
    TextBlock block;
    OcrLine line;
    line.text = input_;
    line.bounds = RectF{0.0F, 0.0F, 1.0F, 1.0F};
    block.lines.push_back(std::move(line));
    block.forced_target_language = target_;

    try {
        effective_target_ = effective_target();
        submitted_configuration_ = configuration_signature();
        std::vector<TextBlock> blocks;
        blocks.push_back(std::move(block));
        request_id_ = pipeline_.schedule_translation(
            std::move(blocks), PipelineOptions::from_config(config_), PipelineLane::text);
        output_.clear();
        error_.clear();
        state_ = TextTranslationState::translating;
    } catch (const std::exception& exception) {
        request_id_ = 0;
        output_.clear();
        error_ = utf8_to_wide(exception.what());
        state_ = TextTranslationState::error;
    }
    notify_changed();
    return state_ == TextTranslationState::translating;
}

bool TextTranslationSession::configuration_changed() {
    if (!has_visible_text() || character_count() > max_characters) return false;
    if (!submitted_configuration_.empty() &&
        submitted_configuration_ == configuration_signature()) {
        return false;
    }
    invalidate(TextTranslationState::waiting);
    notify_changed();
    return true;
}

bool TextTranslationSession::handle_completion(PipelineCompletion completion) {
    if (completion.lane != PipelineLane::text || !request_id_ ||
        completion.request_id != request_id_) {
        return false;
    }
    request_id_ = 0;
    if (!completion.result || completion.result->blocks.size() != 1) {
        output_.clear();
        error_ = completion.error.empty() ? L"文字翻译失败" : std::move(completion.error);
        state_ = TextTranslationState::error;
    } else {
        output_ = std::move(completion.result->blocks.front().translated);
        error_.clear();
        state_ = TextTranslationState::ready;
    }
    notify_changed();
    return true;
}

void TextTranslationSession::cancel() {
    invalidate(has_visible_text() ? TextTranslationState::waiting
                                  : TextTranslationState::idle);
    notify_changed();
}

void TextTranslationSession::self_test() {
    set_target({});
    set_input(std::wstring(max_characters, L'a'));
    if (character_count() != max_characters || !can_submit() ||
        state_ != TextTranslationState::waiting) {
        throw AppError("text session self-test rejected the 5000 character boundary");
    }
    set_input(std::wstring(max_characters + 1, L'a'));
    if (can_submit() || state_ != TextTranslationState::too_long) {
        throw AppError("text session self-test accepted more than 5000 characters");
    }
    std::wstring surrogate_input(max_characters - 1, L'a');
    surrogate_input.push_back(static_cast<wchar_t>(0xD83D));
    surrogate_input.push_back(static_cast<wchar_t>(0xDE00));
    set_input(std::move(surrogate_input));
    if (character_count() != max_characters || !can_submit()) {
        throw AppError("text session self-test counted a surrogate pair twice");
    }

    set_input(L"你好");
    const auto configured_target = target_for(
        input_, config_.string(L"lang.zh_target", L"en"));
    if (effective_target() != configured_target) {
        throw AppError("text session self-test Chinese auto target mismatch");
    }
    set_input(L"hello");
    if (effective_target() != L"zh-Hans") {
        throw AppError("text session self-test non-Chinese auto target mismatch");
    }
    if (!set_target(L"ja") || effective_target() != L"ja" ||
        set_target(L"unsupported")) {
        throw AppError("text session self-test manual target mismatch");
    }

    request_id_ = 41;
    state_ = TextTranslationState::translating;
    PipelineResult stale_result;
    stale_result.blocks.push_back(BlockTranslation{{}, L"stale", L"ja"});
    if (handle_completion(PipelineCompletion{
            40, PipelineLane::text, std::move(stale_result), {},
        }) || request_id_ != 41 || state_ != TextTranslationState::translating) {
        throw AppError("text session self-test accepted a late result");
    }

    if (!handle_completion(PipelineCompletion{
            41, PipelineLane::text, std::nullopt, L"test failure",
        }) || state_ != TextTranslationState::error || error_ != L"test failure") {
        throw AppError("text session self-test error state mismatch");
    }
    request_id_ = 42;
    state_ = TextTranslationState::translating;
    PipelineResult recovered;
    recovered.blocks.push_back(BlockTranslation{{}, L"recovered", L"ja"});
    if (!handle_completion(PipelineCompletion{
            42, PipelineLane::text, std::move(recovered), {},
        }) || state_ != TextTranslationState::ready || output_ != L"recovered" ||
        !error_.empty()) {
        throw AppError("text session self-test did not recover after an error");
    }
    set_target({});
    clear();
}

}  // namespace screentrans
