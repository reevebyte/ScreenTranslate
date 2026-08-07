#pragma once

#include "config.hpp"
#include "text_translation_session.hpp"

#include <windows.h>

#include <functional>
#include <string>

namespace screentrans {

class TextTranslateWindow {
public:
    TextTranslateWindow(HINSTANCE instance, ConfigStore& config,
                        TextTranslationSession& session);
    ~TextTranslateWindow();

    TextTranslateWindow(const TextTranslateWindow&) = delete;
    TextTranslateWindow& operator=(const TextTranslateWindow&) = delete;

    void show(HWND owner);
    void hide();
    void refresh();
    void refresh_appearance();
    void self_test(HWND owner);
    [[nodiscard]] bool preprocess_message(MSG& message);
    [[nodiscard]] bool visible() const noexcept;
    void set_callbacks(TextTranslationCallbacks callbacks) {
        callbacks_ = std::move(callbacks);
    }

private:
    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    static LRESULT CALLBACK input_proc(HWND control, UINT message,
                                       WPARAM wparam, LPARAM lparam,
                                       UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK target_combo_proc(HWND control, UINT message,
                                              WPARAM wparam, LPARAM lparam,
                                              UINT_PTR subclass_id,
                                              DWORD_PTR reference);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    void create_window(HWND owner);
    void create_controls();
    void create_theme_resources();
    void destroy_theme_resources() noexcept;
    void layout_controls();
    void position_initial(HWND owner);
    void sync_input();
    void sync_target();
    void on_input_changed();
    void on_target_changed();
    LRESULT draw_item(const DRAWITEMSTRUCT& item);
    void draw_target_combo(HDC dc);
    void paint();

    HINSTANCE instance_{};
    ConfigStore& config_;
    TextTranslationSession& session_;
    TextTranslationCallbacks callbacks_;
    HWND window_{};
    HWND source_label_{};
    HWND target_combo_{};
    HWND input_{};
    HWND output_{};
    HWND counter_{};
    HWND status_{};
    HWND clear_button_{};
    HWND copy_button_{};
    HWND settings_button_{};
    HFONT font_{};
    HFONT title_font_{};
    HFONT small_font_{};
    HBRUSH background_{};
    HBRUSH surface_{};
    HBRUSH input_background_{};
    COLORREF accent_{};
    int dpi_{96};
    bool syncing_{};
    bool status_error_{};
};

class QuickTranslateWindow {
public:
    QuickTranslateWindow(HINSTANCE instance, ConfigStore& config,
                         TextTranslationSession& session);
    ~QuickTranslateWindow();

    QuickTranslateWindow(const QuickTranslateWindow&) = delete;
    QuickTranslateWindow& operator=(const QuickTranslateWindow&) = delete;

    void toggle(HWND owner);
    void show(HWND owner);
    void hide();
    void refresh();
    void refresh_appearance();
    void self_test(HWND owner);
    [[nodiscard]] bool preprocess_message(MSG& message);
    [[nodiscard]] bool visible() const noexcept;
    void set_callbacks(TextTranslationCallbacks callbacks) {
        callbacks_ = std::move(callbacks);
    }

private:
    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    static LRESULT CALLBACK input_proc(HWND control, UINT message,
                                       WPARAM wparam, LPARAM lparam,
                                       UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK target_combo_proc(HWND control, UINT message,
                                              WPARAM wparam, LPARAM lparam,
                                              UINT_PTR subclass_id,
                                              DWORD_PTR reference);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    void create_window(HWND owner);
    void create_controls();
    void create_theme_resources();
    void destroy_theme_resources() noexcept;
    void layout_controls();
    void position_on_active_monitor();
    void sync_input();
    void sync_target();
    void on_input_changed();
    void on_target_changed();
    LRESULT draw_item(const DRAWITEMSTRUCT& item);
    void draw_target_combo(HDC dc);
    void paint();

    HINSTANCE instance_{};
    ConfigStore& config_;
    TextTranslationSession& session_;
    TextTranslationCallbacks callbacks_;
    HWND window_{};
    HWND target_combo_{};
    HWND input_{};
    HWND output_{};
    HWND copy_button_{};
    HWND settings_button_{};
    HWND close_button_{};
    HFONT font_{};
    HFONT title_font_{};
    HFONT small_font_{};
    HFONT icon_font_{};
    HBRUSH background_{};
    HBRUSH surface_{};
    HBRUSH input_background_{};
    COLORREF accent_{};
    int dpi_{96};
    bool syncing_{};
    bool status_error_{};
};

}  // namespace screentrans
