#pragma once

#include "config.hpp"

#include <windows.h>

#include <array>
#include <cstdint>
#include <functional>
#include <stop_token>
#include <string>
#include <thread>
#include <vector>

namespace screentrans {

class SettingsWindow {
public:
    SettingsWindow(HINSTANCE instance, ConfigStore& config);
    ~SettingsWindow();

    SettingsWindow(const SettingsWindow&) = delete;
    SettingsWindow& operator=(const SettingsWindow&) = delete;

    void show(HWND owner);
    void self_test(HWND owner);
    [[nodiscard]] bool preprocess_message(MSG& message);
    void set_hotkeys_changed_callback(std::function<bool()> callback) {
        hotkeys_changed_callback_ = std::move(callback);
    }
    void set_restart_callback(std::function<void()> callback) {
        restart_callback_ = std::move(callback);
    }
    void set_settings_changed_callback(std::function<void()> callback) {
        settings_changed_callback_ = std::move(callback);
    }

private:
    struct ModelListResult {
        std::uint64_t generation{};
        std::wstring provider;
        std::vector<std::wstring> models;
        std::wstring error;
        bool cancelled{};
    };

    static constexpr UINT test_completed_message = WM_APP + 61;
    static constexpr UINT models_completed_message = WM_APP + 62;
    static constexpr UINT clear_combo_selection_message = WM_APP + 63;

    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    static LRESULT CALLBACK combo_proc(HWND control, UINT message,
                                       WPARAM wparam, LPARAM lparam,
                                       UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK combo_edit_proc(HWND control, UINT message,
                                            WPARAM wparam, LPARAM lparam,
                                            UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK edit_proc(HWND control, UINT message,
                                      WPARAM wparam, LPARAM lparam,
                                      UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK hotkey_proc(HWND control, UINT message,
                                        WPARAM wparam, LPARAM lparam,
                                        UINT_PTR subclass_id, DWORD_PTR reference);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    void create_window(HWND owner);
    void create_controls();
    void create_hotkey_page();
    void create_translation_page();
    void create_ocr_page();
    void create_appearance_page();
    void create_other_page();
    void create_theme_resources();
    void apply_control_theme(HWND control);
    void layout_controls();
    void show_page(int index);
    void paint();
    void draw_combo(HWND control, HDC dc);
    LRESULT draw_item(const DRAWITEMSTRUCT& item);
    bool is_checkbox(HWND control) const noexcept;
    bool checkbox_checked(HWND control) const noexcept;
    void set_checkbox(HWND control, bool checked);
    HWND add_control(int page, DWORD ex_style, const wchar_t* type,
                     const wchar_t* text, DWORD style, int identifier = 0);
    HWND add_label(int page, const wchar_t* text);
    void load_values();
    bool save_values(bool show_error = false);
    void save_provider_fields();
    void refresh_provider_fields(bool load);
    void refresh_ocr_fields();
    void refresh_close_fields();
    void start_connection_test();
    void finish_connection_test(bool success, std::wstring message);
    void discard_connection_test_results(HWND target = nullptr) noexcept;
    void start_model_refresh();
    void finish_model_refresh(ModelListResult result);
    void cancel_model_refresh(bool clear_status = false) noexcept;
    void discard_model_refresh_results(HWND target = nullptr) noexcept;
    void set_status(std::wstring_view message, bool error = false);
    void close();

    HINSTANCE instance_{};
    ConfigStore& config_;
    HWND owner_{};
    HWND window_{};
    std::array<HWND, 5> navigation_{};
    HWND status_{};
    HWND side_footer_{};
    HWND capture_hotkey_{};
    HWND toggle_hotkey_{};
    HWND provider_{};
    HWND provider_key_label_{};
    HWND provider_key_{};
    HWND provider_extra_label_{};
    HWND provider_extra_{};
    HWND provider_endpoint_label_{};
    HWND provider_endpoint_{};
    HWND provider_model_label_{};
    HWND provider_model_{};
    HWND model_refresh_{};
    HWND deepl_free_{};
    HWND test_button_{};
    HWND test_status_{};
    HWND chinese_target_{};
    HWND ocr_engine_{};
    HWND ocr_cloud_note_{};
    HWND ocr_languages_{};
    HWND ocr_upscale_{};
    HWND azure_endpoint_label_{};
    HWND azure_endpoint_{};
    HWND azure_key_label_{};
    HWND azure_key_{};
    HWND font_family_{};
    HWND accent_{};
    HWND close_mode_{};
    HWND timeout_label_{};
    HWND timeout_seconds_{};
    HWND auto_copy_{};
    HWND autostart_{};
    HWND config_path_{};
    HWND open_config_{};
    HWND restart_{};
    HWND diagnostic_command_{};
    HFONT font_{};
    HFONT title_font_{};
    HFONT brand_font_{};
    HFONT small_font_{};
    HFONT icon_font_{};
    HBRUSH background_{};
    HBRUSH sidebar_background_{};
    HBRUSH card_background_{};
    HBRUSH input_background_{};
    std::array<std::vector<HWND>, 5> page_controls_;
    std::vector<std::vector<std::wstring>> ocr_language_options_;
    std::string window_error_;
    std::wstring loaded_provider_;
    int dpi_{96};
    int current_page_{};
    bool finished_{};
    bool loading_{};
    bool status_error_{};
    bool test_success_{};
    bool test_pending_{};
    bool model_refreshing_{};
    std::jthread test_thread_;
    std::jthread model_thread_;
    std::uint64_t model_generation_{};
    std::function<bool()> hotkeys_changed_callback_;
    std::function<void()> restart_callback_;
    std::function<void()> settings_changed_callback_;
};

}  // namespace screentrans
