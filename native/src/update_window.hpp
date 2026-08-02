#pragma once

#include "config.hpp"
#include "updater.hpp"

#include <windows.h>

#include <atomic>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <thread>

namespace screentrans {

class UpdateWindow {
public:
    UpdateWindow(HINSTANCE instance, ConfigStore& config);
    ~UpdateWindow();

    UpdateWindow(const UpdateWindow&) = delete;
    UpdateWindow& operator=(const UpdateWindow&) = delete;

    void show(HWND owner);
    void self_test(HWND owner);
    void refresh_appearance();
    [[nodiscard]] bool preprocess_message(MSG& message);
    void set_quit_callback(std::function<void()> callback) {
        quit_callback_ = std::move(callback);
    }
    void set_update_available_callback(std::function<void(const UpdateInfo&)> callback) {
        update_available_callback_ = std::move(callback);
    }

private:
    enum class WorkerMode { none, check, download };
    enum class ViewState {
        idle,
        checking,
        current,
        available,
        downloading,
        ready,
        installing,
        error,
        unconfigured,
    };
    enum class ButtonRole { primary, ghost };
    struct CheckResult {
        std::optional<UpdateInfo> update;
        std::wstring error;
        bool cancelled{};
    };
    struct DownloadResult {
        std::filesystem::path path;
        std::wstring error;
        bool cancelled{};
    };
    struct WorkerDispatch;

    static constexpr UINT check_completed_message = WM_APP + 70;
    static constexpr UINT download_completed_message = WM_APP + 71;
    static constexpr UINT progress_message = WM_APP + 72;
    static constexpr UINT install_prompt_message = WM_APP + 73;

    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    void create_window(HWND owner);
    void create_controls();
    void create_theme_resources();
    void layout_controls();
    void sync_text_controls();
    void refresh_repository_control();
    void resize_for_content();
    void paint();
    LRESULT draw_item(const DRAWITEMSTRUCT& item);
    void start_check();
    void start_download();
    void install();
    void open_release();
    void finish_worker();
    void discard_worker_messages(HWND target = nullptr) noexcept;
    void set_state(ViewState state, std::wstring title, std::wstring message);
    void set_state_message(std::wstring message);
    void set_details_visible(bool visible);
    void close();
    void shutdown_worker(DWORD timeout_ms) noexcept;
    void invalidate_worker_dispatch() noexcept;
    [[nodiscard]] static bool post_worker_message(
        const std::shared_ptr<WorkerDispatch>& dispatch,
        UINT message, LPARAM lparam) noexcept;

    HINSTANCE instance_{};
    ConfigStore& config_;
    HWND owner_{};
    HWND window_{};
    HWND check_button_{};
    HWND release_button_{};
    HWND action_button_{};
    HWND state_message_control_{};
    HWND published_value_control_{};
    HWND artifact_value_control_{};
    HWND source_control_{};
    HWND repository_tooltip_{};
    HFONT font_{};
    HFONT title_font_{};
    HFONT state_font_{};
    HFONT small_font_{};
    HBRUSH background_{};
    HBRUSH card_background_{};
    std::wstring state_title_;
    std::wstring state_message_;
    std::wstring published_value_;
    std::wstring artifact_value_;
    std::wstring verification_value_;
    std::wstring repository_tooltip_text_;
    std::string window_error_;
    std::optional<UpdateInfo> available_;
    std::optional<std::filesystem::path> downloaded_;
    std::jthread worker_;
    std::shared_ptr<std::atomic_bool> worker_finished_;
    std::shared_ptr<WorkerDispatch> worker_dispatch_;
    WorkerMode worker_mode_{WorkerMode::none};
    ViewState view_state_{ViewState::idle};
    ButtonRole check_role_{ButtonRole::primary};
    ButtonRole release_role_{ButtonRole::ghost};
    ButtonRole action_role_{ButtonRole::primary};
    std::uint64_t progress_received_{};
    std::uint64_t progress_total_{};
    int dpi_{96};
    bool details_visible_{};
    bool progress_visible_{};
    bool pending_install_prompt_{};
    std::function<void()> quit_callback_;
    std::function<void(const UpdateInfo&)> update_available_callback_;
};

}  // namespace screentrans
