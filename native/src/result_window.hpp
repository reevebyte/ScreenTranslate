#pragma once

#include "pipeline.hpp"
#include "result_renderer.hpp"

#include <windows.h>

#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace screentrans {

struct ResultAppearance {
    std::wstring font_family{L"Microsoft YaHei UI"};
    int minimum_font_pixels{9};
    COLORREF accent{RGB(40, 199, 111)};
    std::wstring close_mode{L"click"};
    int timeout_ms{5000};
};

class ResultWindow {
public:
    explicit ResultWindow(HINSTANCE instance);
    ~ResultWindow();

    ResultWindow(const ResultWindow&) = delete;
    ResultWindow& operator=(const ResultWindow&) = delete;

    void show_loading(const RECT& physical_rect,
                      std::shared_ptr<const PixelBuffer> original,
                      ResultAppearance appearance);
    void show_retry_loading(ResultAppearance appearance);
    void set_result(PipelineResult result);
    void set_error(std::wstring message);
    void close();
    void minimize();
    void restore();
    void set_editing(bool value);
    [[nodiscard]] bool visible() const noexcept;
    [[nodiscard]] HWND handle() const noexcept { return window_; }
    [[nodiscard]] const std::wstring& plain_text() const noexcept { return plain_text_; }

    void set_copy_callback(std::function<void(std::wstring_view)> callback) {
        copy_callback_ = std::move(callback);
    }
    void set_retry_callback(std::function<void()> callback) { retry_callback_ = std::move(callback); }
    void set_edit_callback(std::function<void()> callback) { edit_callback_ = std::move(callback); }
    void set_closed_callback(std::function<void()> callback) { closed_callback_ = std::move(callback); }
    void set_recapture_callback(std::function<void(const RECT&)> callback) {
        recapture_callback_ = std::move(callback);
    }

private:
    struct PendingSource {
        RECT rect{};
        std::shared_ptr<const PixelBuffer> original;
        ResultAppearance appearance;
        POINT image_origin{};
    };

    static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam);
    static LRESULT CALLBACK control_bar_proc(HWND window, UINT message,
                                             WPARAM wparam, LPARAM lparam);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    LRESULT handle_control_bar_message(UINT message, WPARAM wparam, LPARAM lparam);
    void paint();
    void create_window();
    void create_control_bar();
    void destroy_control_bar();
    void paint_control_bar();
    void sync_control_bar();
    void update_control_bar_region();
    void update_window_region();
    [[nodiscard]] HDC ensure_paint_buffer(HDC target, int width, int height);
    void release_paint_buffer() noexcept;
    [[nodiscard]] RECT control_bar_button_rect(int index) const;
    [[nodiscard]] int control_bar_button_at(POINT point) const;
    void build_repaired_background();
    void set_peek(bool value);
    void copy_all();
    void move_by(int dx, int dy);
    void reset_geometry();
    void freeze_current_surface() noexcept;
    void aim_handles(double target);
    bool set_capture_exclusion(bool exclude) noexcept;
    void arm_close_timer();

    HINSTANCE instance_{};
    HWND window_{};
    HWND control_bar_{};
    HWND control_bar_tooltip_{};
    HDC paint_buffer_dc_{};
    HBITMAP paint_buffer_bitmap_{};
    HGDIOBJ paint_buffer_original_{};
    int paint_buffer_width_{};
    int paint_buffer_height_{};
    std::shared_ptr<const PixelBuffer> original_;
    std::optional<PendingSource> pending_source_;
    PixelBuffer refresh_surface_;
    PixelBuffer translated_surface_;
    PipelineResult result_;
    std::wstring plain_text_;
    std::wstring error_;
    ResultAppearance appearance_;
    RECT home_rect_{};
    RECT captured_rect_{};
    RECT resize_start_rect_{};
    POINT image_screen_origin_{};
    POINT control_bar_drag_cursor_{};
    POINT content_drag_cursor_{};
    int minimum_width_{1};
    int minimum_height_{1};
    int control_bar_hover_{-1};
    int control_bar_pressed_{-1};
    double handles_alpha_{};
    double progress_phase_{};
    bool loading_{};
    bool peek_{};
    bool closing_{};
    bool successful_{};
    bool editing_{};
    bool editable_result_{};
    bool home_initialized_{};
    bool resizing_{};
    bool recapturing_{};
    bool refreshing_{};
    bool refresh_was_active_on_resize_{};
    bool recapture_exclusion_active_{};
    bool control_bar_capture_excluded_{};
    bool control_bar_dragging_{};
    bool content_dragging_{};
    bool freeze_content_only_{};
    std::function<void(std::wstring_view)> copy_callback_;
    std::function<void()> retry_callback_;
    std::function<void()> edit_callback_;
    std::function<void()> closed_callback_;
    std::function<void(const RECT&)> recapture_callback_;
};

}  // namespace screentrans
