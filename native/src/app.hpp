#pragma once

#include "config.hpp"
#include "pipeline.hpp"
#include "result_window.hpp"
#include "selection_overlay.hpp"
#include "settings_window.hpp"
#include "update_window.hpp"

#include <windows.h>
#include <shellapi.h>

#include <cstddef>
#include <cstdint>
#include <memory>
#include <mutex>
#include <optional>
#include <thread>
#include <vector>

namespace screentrans {

class BlockEditor;

class AppHost {
public:
    explicit AppHost(HINSTANCE instance);
    ~AppHost();

    AppHost(const AppHost&) = delete;
    AppHost& operator=(const AppHost&) = delete;

    int run(bool self_test = false, bool update_self_test = false);
    [[nodiscard]] HWND handle() const noexcept { return window_; }
    static constexpr const wchar_t* window_class_name() noexcept {
        return L"ScreenTranslate.Native.MessageWindow.v1";
    }
    static constexpr UINT activate_message = WM_APP + 1;

private:
    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    void create_message_window();
    void add_tray_icon();
    void remove_tray_icon();
    void refresh_tray_icon();
    void refresh_tray_tooltip();
    void show_tray_menu(POINT location);
    void notify(std::wstring_view title, std::wstring_view message);
    bool apply_hotkeys(bool announce_changes);
    void start_capture();
    void begin_capture_now();
    void on_selected(const RECT& rect, PixelBuffer image);
    void start_pipeline(std::shared_ptr<const PixelBuffer> image, const RECT& rect);
    void process_pipeline_completions();
    void toggle_result();
    void copy_to_clipboard(std::wstring_view text);
    void retry_translation();
    void open_block_editor();
    void request_block_translation(BlockEditor& editor, std::size_t index,
                                   TextBlock block);
    void cancel_block_translation(BlockEditor& editor) noexcept;
    bool schedule_block_translations(std::vector<std::size_t> indices,
                                     std::vector<TextBlock> blocks,
                                     BlockEditor* editor);
    void process_block_translation(PipelineCompletion completion);
    void refresh_cached_result();
    void request_recapture(const RECT& rect);
    void recapture_now();
    void open_settings();
    void open_updates();
    void start_silent_update_check();
    std::optional<UpdateInfo> process_silent_update_check();
    void handle_update_available(const UpdateInfo& update);
    void restart();
    void quit();
    ResultAppearance result_appearance() const;

    HINSTANCE instance_{};
    HWND window_{};
    UINT taskbar_created_message_{};
    NOTIFYICONDATAW tray_{};
    HICON icon_{};
    std::wstring tray_accent_;
    ConfigStore config_;
    SettingsWindow settings_;
    UpdateWindow updates_;
    SelectionOverlay overlay_;
    ResultWindow result_;
    std::unique_ptr<PipelineController> pipeline_;
    struct BlockTranslationRequest {
        std::uint64_t request_id{};
        std::vector<std::size_t> indices;
        BlockEditor* editor{};
    };
    std::uint64_t current_request_{};
    std::optional<BlockTranslationRequest> block_translation_request_;
    std::shared_ptr<const PixelBuffer> last_image_;
    std::vector<TextBlock> last_blocks_;
    std::optional<PipelineResult> last_result_;
    RECT last_rect_{};
    std::optional<RECT> pending_recapture_;
    std::jthread silent_update_thread_;
    std::mutex silent_update_mutex_;
    std::optional<UpdateInfo> silent_update_result_;
    std::wstring registered_capture_hotkey_;
    std::wstring registered_toggle_hotkey_;
    std::wstring available_update_version_;
    bool shutting_down_{};
    bool self_test_mode_{};
    bool update_self_test_mode_{};
};

}  // namespace screentrans
