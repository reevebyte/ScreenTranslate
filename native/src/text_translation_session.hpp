#pragma once

#include "config.hpp"
#include "pipeline.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <string>
#include <string_view>

namespace screentrans {

enum class TextTranslationState {
    idle,
    waiting,
    translating,
    ready,
    error,
    too_long,
};

struct TextTranslationCallbacks {
    std::function<void(std::wstring)> input_changed;
    std::function<void(std::wstring)> target_changed;
    std::function<void(bool)> composition_changed;
    std::function<void()> translate_now;
    std::function<void()> clear;
    std::function<void()> copy;
    std::function<void()> open_settings;
};

class TextTranslationSession {
public:
    static constexpr std::size_t max_characters = 5000;

    TextTranslationSession(ConfigStore& config, PipelineController& pipeline);

    bool set_input(std::wstring value);
    bool set_target(std::wstring value);
    void clear();
    bool submit();
    bool configuration_changed();
    bool handle_completion(PipelineCompletion completion);
    void cancel();
    void self_test();

    void set_changed_callback(std::function<void()> callback) {
        changed_callback_ = std::move(callback);
    }

    [[nodiscard]] const std::wstring& input() const noexcept { return input_; }
    [[nodiscard]] const std::wstring& output() const noexcept { return output_; }
    [[nodiscard]] const std::wstring& error() const noexcept { return error_; }
    [[nodiscard]] const std::wstring& target() const noexcept { return target_; }
    [[nodiscard]] TextTranslationState state() const noexcept { return state_; }
    [[nodiscard]] std::size_t character_count() const noexcept;
    [[nodiscard]] std::wstring effective_target() const;
    [[nodiscard]] bool can_submit() const noexcept;
    [[nodiscard]] std::uint64_t request_id() const noexcept { return request_id_; }

private:
    [[nodiscard]] bool has_visible_text() const noexcept;
    [[nodiscard]] std::wstring configuration_signature() const;
    void invalidate(TextTranslationState next_state);
    void notify_changed();

    ConfigStore& config_;
    PipelineController& pipeline_;
    std::wstring input_;
    std::wstring output_;
    std::wstring error_;
    std::wstring target_;
    std::wstring effective_target_;
    std::wstring submitted_configuration_;
    TextTranslationState state_{TextTranslationState::idle};
    std::uint64_t request_id_{};
    std::function<void()> changed_callback_;
};

const std::array<std::wstring_view, 9>& text_translation_targets() noexcept;

}  // namespace screentrans
