#pragma once

#include "pipeline.hpp"

#include <windows.h>

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <vector>

namespace screentrans {

class BlockEditor {
public:
    struct Change {
        std::size_t index{};
        TextBlock block;
    };

    using RetranslateCallback = std::function<void(BlockEditor&, Change)>;
    using CancelRequestCallback = std::function<void(BlockEditor&)>;
    using CompletionCallback = std::function<void(
        std::optional<std::vector<Change>>)>;

    static bool show(
        HINSTANCE instance,
        HWND owner,
        const PipelineResult& result,
        std::wstring_view chinese_target,
        RetranslateCallback retranslate_callback,
        CancelRequestCallback cancel_request_callback,
        CompletionCallback completion_callback,
        std::wstring_view accent = L"#28C76F");

    static void close_active() noexcept;
    [[nodiscard]] static bool preprocess_active_message(MSG& message) noexcept;
    static void self_test(HINSTANCE instance, HWND owner);
    void set_translation(std::size_t index, const BlockTranslation& translation);
    void set_error(std::size_t index, std::wstring message);

private:
    struct Draft {
        TextBlock block;
        std::wstring source;
        std::wstring translation;
        std::wstring actual_target;
        std::wstring target_mode;
        std::wstring applied_source;
        std::wstring applied_target_mode;
        std::optional<std::wstring> submitted_source;
        std::optional<std::wstring> submitted_target_mode;
        std::wstring error;
        bool busy{};
        bool success{};

        [[nodiscard]] bool dirty() const noexcept {
            return source != applied_source || target_mode != applied_target_mode;
        }
    };

    BlockEditor(HINSTANCE instance, HWND owner, const PipelineResult& result,
                std::wstring_view chinese_target,
                RetranslateCallback retranslate_callback,
                CancelRequestCallback cancel_request_callback,
                CompletionCallback completion_callback,
                std::wstring_view accent);
    ~BlockEditor();

    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam);
    static LRESULT CALLBACK combo_proc(HWND control, UINT message,
                                       WPARAM wparam, LPARAM lparam,
                                       UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK edit_proc(HWND control, UINT message,
                                      WPARAM wparam, LPARAM lparam,
                                      UINT_PTR subclass_id, DWORD_PTR reference);
    static LRESULT CALLBACK list_proc(HWND control, UINT message,
                                      WPARAM wparam, LPARAM lparam,
                                      UINT_PTR subclass_id, DWORD_PTR reference);
    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam);
    void create_window();
    void create_controls();
    void create_theme_resources();
    void apply_control_theme(HWND control);
    void layout_controls();
    void populate_blocks();
    void update_list_item(int index);
    void create_list_tooltip();
    void destroy_list_tooltip() noexcept;
    void refresh_list_hover();
    void set_list_hover(int index);
    [[nodiscard]] bool list_item_is_truncated(int index) const;
    void load_current(int index);
    void source_changed();
    void target_changed();
    void refresh_current_state();
    void refresh_apply_button();
    bool validate_drafts(bool current_only);
    [[nodiscard]] Change make_change(std::size_t index) const;
    void notify_request_cancelled() noexcept;
    void accept();
    void accept_current();
    void cancel() noexcept;
    void complete(std::optional<std::vector<Change>> changes) noexcept;
    void paint();
    void draw_combo(HWND control, HDC dc);
    LRESULT draw_item(const DRAWITEMSTRUCT& item);
    void present();

    [[nodiscard]] int scale(int value) const noexcept;
    [[nodiscard]] bool current_source_empty() const;

    HINSTANCE instance_{};
    HWND owner_{};
    HWND window_{};
    HWND list_{};
    HWND list_tooltip_{};
    HWND source_{};
    HWND target_{};
    HWND translation_{};
    HWND retranslate_{};
    HWND close_{};
    HWND apply_remaining_{};

    HFONT font_{};
    HFONT title_font_{};
    HFONT block_title_font_{};
    HFONT small_font_{};
    HFONT primary_font_{};
    HBRUSH background_{};
    HBRUSH card_background_{};
    HBRUSH input_background_{};
    HBRUSH translation_background_{};

    RECT title_rect_{};
    RECT list_rect_{};
    RECT card_rect_{};
    RECT block_title_rect_{};
    RECT confidence_rect_{};
    RECT source_label_rect_{};
    RECT source_shell_rect_{};
    RECT target_label_rect_{};
    RECT target_combo_rect_{};
    RECT translation_label_rect_{};
    RECT translation_shell_rect_{};
    RECT status_rect_{};

    std::vector<Draft> drafts_;
    RetranslateCallback retranslate_callback_;
    CancelRequestCallback cancel_request_callback_;
    CompletionCallback completion_callback_;
    std::wstring chinese_target_;
    std::wstring accent_text_;
    std::wstring block_heading_;
    std::wstring confidence_text_;
    std::wstring status_text_;
    std::wstring list_tooltip_text_;
    std::string window_error_;
    COLORREF confidence_color_{};
    COLORREF status_color_{};
    COLORREF accent_color_{};
    int dpi_{96};
    int current_{-1};
    int list_hover_{-1};
    bool presented_{};
    bool finished_{};
    bool loading_{};
    bool close_notified_{};
    bool list_mouse_tracking_{};
};

}  // namespace screentrans
