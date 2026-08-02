#include "block_editor.hpp"

#include "language.hpp"
#include "util.hpp"

#include <commctrl.h>
#include <dwmapi.h>
#include <shellscalingapi.h>
#include <uxtheme.h>
#include <windowsx.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cwctype>
#include <string_view>

namespace screentrans {

namespace {

constexpr wchar_t class_name[] = L"ScreenTranslate.Native.BlockEditor.v2";
constexpr int id_list = 6101;
constexpr int id_source = 6102;
constexpr int id_target = 6103;
constexpr int id_translation = 6104;
constexpr int id_retranslate = 6105;
constexpr int id_apply_remaining = 6106;
constexpr UINT_PTR combo_subclass_id = 1;
constexpr UINT_PTR edit_subclass_id = 2;
constexpr UINT_PTR list_subclass_id = 3;

constexpr COLORREF color_page = RGB(21, 22, 27);
constexpr COLORREF color_card = RGB(27, 29, 35);
constexpr COLORREF color_input = RGB(33, 36, 41);
constexpr COLORREF color_translation = RGB(25, 27, 32);
constexpr COLORREF color_line = RGB(40, 43, 51);
constexpr COLORREF color_line_high = RGB(52, 56, 66);
constexpr COLORREF color_text = RGB(231, 233, 236);
constexpr COLORREF color_text_dim = RGB(148, 154, 164);
constexpr COLORREF color_text_faint = RGB(110, 116, 126);
constexpr COLORREF color_warning = RGB(232, 180, 74);
constexpr COLORREF color_bad = RGB(255, 107, 107);

constexpr std::array<std::pair<std::wstring_view, std::wstring_view>, 9> targets{{
    {L"zh-Hans", L"简体中文"}, {L"en", L"英语"}, {L"ja", L"日语"},
    {L"ko", L"韩语"}, {L"fr", L"法语"}, {L"de", L"德语"},
    {L"es", L"西班牙语"}, {L"ru", L"俄语"}, {L"zh-Hant", L"繁体中文"},
}};

BlockEditor* active_editor{};

int scaled(int value, int dpi) noexcept {
    return MulDiv(value, dpi, 96);
}

std::wstring control_text(HWND control) {
    const int length = GetWindowTextLengthW(control);
    std::wstring value(static_cast<std::size_t>(std::max(0, length)) + 1, L'\0');
    if (length > 0) GetWindowTextW(control, value.data(), length + 1);
    value.resize(static_cast<std::size_t>(std::max(0, length)));
    return value;
}

std::wstring preview(std::wstring_view value, std::size_t limit = 32,
                     bool* was_truncated = nullptr) {
    std::wstring one_line;
    one_line.reserve(value.size());
    bool pending_space = false;
    for (const wchar_t ch : value) {
        if (std::iswspace(ch)) {
            pending_space = !one_line.empty();
            continue;
        }
        if (pending_space) one_line.push_back(L' ');
        pending_space = false;
        one_line.push_back(ch);
    }
    if (one_line.empty()) {
        if (was_truncated) *was_truncated = false;
        return L"（空白）";
    }
    const bool truncated = one_line.size() > limit;
    if (was_truncated) *was_truncated = truncated;
    if (!truncated) return one_line;
    std::wstring output = one_line.substr(0, limit - 1);
    output.push_back(L'…');
    return output;
}

void set_font(HWND control, HFONT font) {
    if (control) SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
}

void fill_round_rect(HDC dc, const RECT& rect, int radius,
                     COLORREF fill, COLORREF border) {
    HBRUSH brush = CreateSolidBrush(fill);
    HPEN pen = CreatePen(PS_SOLID, 1, border);
    if (!brush || !pen) {
        if (pen) DeleteObject(pen);
        if (brush) DeleteObject(brush);
        return;
    }
    const auto old_brush = SelectObject(dc, brush);
    const auto old_pen = SelectObject(dc, pen);
    RoundRect(dc, rect.left, rect.top, rect.right, rect.bottom,
              radius * 2, radius * 2);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
}

void draw_text(HDC dc, std::wstring_view text, RECT rect, HFONT font,
               COLORREF color, UINT format) {
    const auto old_font = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, color);
    DrawTextW(dc, text.data(), static_cast<int>(text.size()), &rect,
              format | DT_NOPREFIX);
    SelectObject(dc, old_font);
}

void enable_dark_frame(HWND window) noexcept {
    constexpr DWORD immersive_dark_mode = 20;
    constexpr DWORD border_color = 34;
    constexpr DWORD caption_color = 35;
    constexpr DWORD caption_text_color = 36;
    BOOL enabled = TRUE;
    DwmSetWindowAttribute(window, immersive_dark_mode, &enabled, sizeof(enabled));
    const COLORREF border = color_line_high;
    const COLORREF caption = RGB(16, 17, 22);
    const COLORREF caption_text = color_text;
    DwmSetWindowAttribute(window, border_color, &border, sizeof(border));
    DwmSetWindowAttribute(window, caption_color, &caption, sizeof(caption));
    DwmSetWindowAttribute(window, caption_text_color, &caption_text, sizeof(caption_text));
}

std::optional<double> block_confidence(const TextBlock& block) {
    double weighted = 0.0;
    std::size_t total = 0;
    for (const auto& line : block.lines) {
        if (line.confidence < 0.0F) continue;
        const auto weight = static_cast<std::size_t>(std::count_if(
            line.text.begin(), line.text.end(), [](wchar_t ch) {
                return std::iswspace(ch) == 0;
            }));
        const auto effective_weight = std::max<std::size_t>(1, weight);
        weighted += static_cast<double>(line.confidence) * effective_weight;
        total += effective_weight;
    }
    if (total == 0) return std::nullopt;
    return weighted / static_cast<double>(total);
}

RECT inset_rect(RECT rect, int amount) noexcept {
    InflateRect(&rect, -amount, -amount);
    return rect;
}

}  // namespace

BlockEditor::BlockEditor(HINSTANCE instance, HWND owner, const PipelineResult& result,
                         std::wstring_view chinese_target,
                         RetranslateCallback retranslate_callback,
                         CancelRequestCallback cancel_request_callback,
                         CompletionCallback completion_callback,
                         std::wstring_view accent)
    : instance_(instance), owner_(owner), chinese_target_(chinese_target),
      retranslate_callback_(std::move(retranslate_callback)),
      cancel_request_callback_(std::move(cancel_request_callback)),
      completion_callback_(std::move(completion_callback)),
      accent_text_(accent), accent_color_(parse_rgb_color(accent)) {
    drafts_.reserve(result.blocks.size());
    for (const auto& item : result.blocks) {
        const auto source = item.block.text();
        const auto target_mode = item.block.forced_target_language;
        drafts_.push_back(Draft{
            item.block,
            source,
            item.translated,
            item.target_language,
            target_mode,
            source,
            target_mode,
            std::nullopt,
            std::nullopt,
            {},
            false,
            false,
        });
    }
}

BlockEditor::~BlockEditor() {
    notify_request_cancelled();
    if (active_editor == this) active_editor = nullptr;
    destroy_list_tooltip();
    if (list_ && IsWindow(list_)) {
        RemoveWindowSubclass(list_, &BlockEditor::list_proc, list_subclass_id);
    }
    if (window_ && IsWindow(window_)) DestroyWindow(window_);
    for (auto** font : {&font_, &title_font_, &block_title_font_, &small_font_,
                        &primary_font_}) {
        if (*font) DeleteObject(*font);
    }
    for (auto** brush : {&background_, &card_background_, &input_background_,
                         &translation_background_}) {
        if (*brush) DeleteObject(*brush);
    }
}

bool BlockEditor::show(
    HINSTANCE instance, HWND owner, const PipelineResult& result,
    std::wstring_view chinese_target, RetranslateCallback retranslate_callback,
    CancelRequestCallback cancel_request_callback,
    CompletionCallback completion_callback, std::wstring_view accent) {
    if (result.blocks.empty()) return false;
    if (active_editor && active_editor->window_ &&
        IsWindow(active_editor->window_)) {
        ShowWindow(active_editor->window_, SW_SHOW);
        SetWindowPos(active_editor->window_, HWND_TOP, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetForegroundWindow(active_editor->window_);
        return true;
    }
    auto* editor = new BlockEditor(
        instance, owner, result, chinese_target,
        std::move(retranslate_callback), std::move(cancel_request_callback),
        std::move(completion_callback), accent);
    active_editor = editor;
    try {
        editor->present();
        return true;
    } catch (...) {
        if (active_editor == editor) active_editor = nullptr;
        delete editor;
        throw;
    }
}

void BlockEditor::close_active() noexcept {
    if (!active_editor) return;
    active_editor->cancel();
}

bool BlockEditor::preprocess_active_message(MSG& message) noexcept {
    BlockEditor* editor = active_editor;
    const HWND dialog = editor ? editor->window_ : nullptr;
    if (!dialog || !IsWindow(dialog) || !IsWindowVisible(dialog)) return false;
    return IsDialogMessageW(dialog, &message) != FALSE;
}

void BlockEditor::self_test(HINSTANCE instance, HWND owner) {
    if (active_editor) throw AppError("block editor self-test window already exists");

    TextBlock block;
    block.edited_text =
        L"用于验证悬停完整原文提示的较长识别文字内容，长度必须超过列表预览上限。";
    block.has_edited_text = true;
    PipelineResult result;
    result.blocks.push_back(BlockTranslation{block, L"Self-test translation", L"en"});

    BlockEditor editor(instance, owner, result, L"en", {}, {}, {}, L"#28C76F");
    editor.create_window();
    DWORD_PTR subclass_reference{};
    const bool list_subclass_installed = editor.list_ && GetWindowSubclass(
        editor.list_, &BlockEditor::list_proc, list_subclass_id,
        &subclass_reference) != FALSE;
    if (!editor.window_ || !editor.list_ || !editor.source_ || !editor.target_ ||
        !editor.translation_ || !editor.retranslate_ || !editor.close_ ||
        !editor.apply_remaining_ || !editor.list_tooltip_ ||
        !list_subclass_installed ||
        subclass_reference != reinterpret_cast<DWORD_PTR>(&editor) ||
        !editor.list_item_is_truncated(0)) {
        throw AppError("block editor self-test did not create required controls");
    }

    const HWND window = editor.window_;
    if (!DestroyWindow(window)) throw_last_error("destroy block editor self-test");
    if (editor.window_ || editor.list_ || editor.list_tooltip_) {
        throw AppError("block editor self-test did not release window resources");
    }
}

int BlockEditor::scale(int value) const noexcept {
    return scaled(value, dpi_);
}

void BlockEditor::create_window() {
    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.style = CS_HREDRAW | CS_VREDRAW;
    description.lpfnWndProc = &BlockEditor::window_proc;
    description.hInstance = instance_;
    description.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    description.hbrBackground = nullptr;
    description.lpszClassName = class_name;
    if (!RegisterClassExW(&description) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register block editor");
    }

    POINT cursor{};
    GetCursorPos(&cursor);
    HMONITOR monitor = owner_ && IsWindow(owner_)
        ? MonitorFromWindow(owner_, MONITOR_DEFAULTTONEAREST)
        : MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
    UINT monitor_dpi_x = 96;
    UINT monitor_dpi_y = 96;
    if (!monitor || FAILED(GetDpiForMonitor(
            monitor, MDT_EFFECTIVE_DPI, &monitor_dpi_x, &monitor_dpi_y))) {
        monitor_dpi_x = GetDpiForSystem();
    }
    dpi_ = owner_ && IsWindow(owner_)
        ? static_cast<int>(GetDpiForWindow(owner_))
        : static_cast<int>(monitor_dpi_x);

    RECT desired{0, 0, scale(680), scale(480)};
    AdjustWindowRectExForDpi(&desired,
                             WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME,
                             FALSE, WS_EX_TOOLWINDOW, static_cast<UINT>(dpi_));
    const int width = desired.right - desired.left;
    const int height = desired.bottom - desired.top;

    MONITORINFO monitor_info{};
    monitor_info.cbSize = sizeof(monitor_info);
    if (!monitor || !GetMonitorInfoW(monitor, &monitor_info)) {
        monitor_info.rcWork = {0, 0, GetSystemMetrics(SM_CXSCREEN),
                               GetSystemMetrics(SM_CYSCREEN)};
    }
    RECT anchor = monitor_info.rcWork;
    if (owner_ && IsWindow(owner_)) GetWindowRect(owner_, &anchor);
    int x = anchor.left + ((anchor.right - anchor.left) - width) / 2;
    int y = anchor.top + ((anchor.bottom - anchor.top) - height) / 2;
    const int work_left = static_cast<int>(monitor_info.rcWork.left);
    const int work_top = static_cast<int>(monitor_info.rcWork.top);
    const int work_right = static_cast<int>(monitor_info.rcWork.right);
    const int work_bottom = static_cast<int>(monitor_info.rcWork.bottom);
    x = std::clamp(x, work_left, std::max(work_left, work_right - width));
    y = std::clamp(y, work_top, std::max(work_top, work_bottom - height));

    window_ = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        class_name, L"校对识别文字",
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_CLIPCHILDREN,
        x, y, width, height, owner_, nullptr, instance_, this);
    if (!window_) {
        if (!window_error_.empty()) throw AppError(window_error_);
        throw_last_error("create block editor");
    }
    enable_dark_frame(window_);
}

void BlockEditor::create_theme_resources() {
    for (auto** font : {&font_, &title_font_, &block_title_font_, &small_font_,
                        &primary_font_}) {
        if (*font) DeleteObject(*font);
        *font = nullptr;
    }
    for (auto** brush : {&background_, &card_background_, &input_background_,
                         &translation_background_}) {
        if (*brush) DeleteObject(*brush);
        *brush = nullptr;
    }
    const auto make_font = [&](int pixels, int weight) {
        return CreateFontW(-scale(pixels), 0, 0, 0, weight, FALSE, FALSE, FALSE,
                           DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                           CLEARTYPE_QUALITY, DEFAULT_PITCH, L"Microsoft YaHei UI");
    };
    font_ = make_font(13, FW_NORMAL);
    title_font_ = make_font(18, FW_SEMIBOLD);
    block_title_font_ = make_font(14, FW_SEMIBOLD);
    small_font_ = make_font(11, FW_NORMAL);
    primary_font_ = make_font(13, FW_SEMIBOLD);
    background_ = CreateSolidBrush(color_page);
    card_background_ = CreateSolidBrush(color_card);
    input_background_ = CreateSolidBrush(color_input);
    translation_background_ = CreateSolidBrush(color_translation);
    if (!font_ || !title_font_ || !block_title_font_ || !small_font_ ||
        !primary_font_ || !background_ || !card_background_ ||
        !input_background_ || !translation_background_) {
        throw AppError("cannot create block editor theme resources");
    }
}

void BlockEditor::apply_control_theme(HWND control) {
    if (!control) return;
    SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
    wchar_t control_class[32]{};
    GetClassNameW(control, control_class, static_cast<int>(std::size(control_class)));
    if (_wcsicmp(control_class, L"Edit") == 0) {
        SetWindowSubclass(control, &BlockEditor::edit_proc, edit_subclass_id,
                          reinterpret_cast<DWORD_PTR>(this));
    } else if (_wcsicmp(control_class, L"ComboBox") == 0) {
        SendMessageW(control, CB_SETITEMHEIGHT, static_cast<WPARAM>(-1), scale(28));
        SendMessageW(control, CB_SETITEMHEIGHT, 0, scale(28));
        SetWindowSubclass(control, &BlockEditor::combo_proc, combo_subclass_id,
                          reinterpret_cast<DWORD_PTR>(this));
    } else if (_wcsicmp(control_class, L"ListBox") == 0) {
        SendMessageW(control, LB_SETITEMHEIGHT, 0, scale(34));
        SetWindowSubclass(control, &BlockEditor::list_proc, list_subclass_id,
                          reinterpret_cast<DWORD_PTR>(this));
    }
}

void BlockEditor::create_controls() {
    create_theme_resources();
    auto make = [&](DWORD ex_style, const wchar_t* type, const wchar_t* text,
                    DWORD style, int identifier) {
        HWND control = CreateWindowExW(
            ex_style, type, text, WS_CHILD | WS_VISIBLE | style,
            0, 0, 1, 1, window_,
            reinterpret_cast<HMENU>(static_cast<INT_PTR>(identifier)),
            instance_, nullptr);
        if (!control) throw_last_error("create block editor control");
        set_font(control, font_);
        apply_control_theme(control);
        return control;
    };

    list_ = make(0, L"LISTBOX", L"",
                 LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS |
                     LBS_NOINTEGRALHEIGHT | WS_VSCROLL | WS_TABSTOP,
                 id_list);
    create_list_tooltip();
    source_ = make(0, L"EDIT", L"",
                   ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN |
                       WS_VSCROLL | WS_TABSTOP,
                   id_source);
    SendMessageW(source_, EM_SETCUEBANNER, TRUE,
                 reinterpret_cast<LPARAM>(L"输入正确的原文"));
    target_ = make(0, L"COMBOBOX", L"",
                   CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS |
                       WS_VSCROLL | WS_TABSTOP,
                   id_target);
    translation_ = make(0, L"EDIT", L"",
                        ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY |
                            WS_VSCROLL | WS_TABSTOP,
                        id_translation);
    retranslate_ = make(0, L"BUTTON", L"应用并重译",
                        BS_OWNERDRAW | BS_NOTIFY | WS_TABSTOP, id_retranslate);
    close_ = make(0, L"BUTTON", L"关闭",
                  BS_OWNERDRAW | BS_NOTIFY | WS_TABSTOP, IDCANCEL);
    apply_remaining_ = make(0, L"BUTTON", L"应用其余更改",
                            BS_OWNERDRAW | BS_NOTIFY | WS_TABSTOP,
                            id_apply_remaining);
    set_font(retranslate_, primary_font_);
    set_font(apply_remaining_, primary_font_);

    const auto automatic = L"自动（中文→" + target_display_name(chinese_target_) +
                           L"，其他→简体中文）";
    SendMessageW(target_, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(automatic.c_str()));
    for (const auto& [code, label] : targets) {
        static_cast<void>(code);
        SendMessageW(target_, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(label.data()));
    }

    populate_blocks();
    if (!drafts_.empty()) {
        SendMessageW(list_, LB_SETCURSEL, 0, 0);
        load_current(0);
    }
    layout_controls();
}

void BlockEditor::layout_controls() {
    if (!window_) return;
    RECT client{};
    GetClientRect(window_, &client);
    const int margin_x = scale(18);
    const int top = scale(16);
    const int gap = scale(12);
    const int list_width = scale(190);
    const int footer_height = scale(34);
    const int footer_bottom = client.bottom - scale(16);
    const int footer_top = footer_bottom - footer_height;

    title_rect_ = {margin_x, top, client.right - margin_x, top + scale(25)};
    const int body_top = title_rect_.bottom + gap;
    const int body_bottom = footer_top - gap;
    list_rect_ = {margin_x, body_top, margin_x + list_width, body_bottom};
    card_rect_ = {list_rect_.right + gap, body_top,
                  client.right - margin_x, body_bottom};

    const int inner_x = card_rect_.left + scale(14);
    const int inner_right = card_rect_.right - scale(14);
    int y = card_rect_.top + scale(12);
    block_title_rect_ = {inner_x, y, inner_right, y + scale(20)};
    y = block_title_rect_.bottom + scale(8);
    confidence_rect_ = {inner_x, y, inner_right, y + scale(16)};
    y = confidence_rect_.bottom + scale(8);
    source_label_rect_ = {inner_x, y, inner_right, y + scale(16)};
    y = source_label_rect_.bottom + scale(8);

    const int action_height = scale(34);
    const int action_y = card_rect_.bottom - scale(12) - action_height;
    const int target_height = scale(34);
    const int label_height = scale(16);
    const int remaining = std::max(scale(64), action_y - y -
        (scale(8) + target_height + scale(8) + label_height + scale(8) + scale(8)));
    const int source_height = std::max(scale(32), remaining / 2);
    const int translation_height = std::max(scale(32), remaining - source_height);

    source_shell_rect_ = {inner_x, y, inner_right, y + source_height};
    y = source_shell_rect_.bottom + scale(8);
    target_label_rect_ = {inner_x, y, inner_x + scale(72), y + target_height};
    target_combo_rect_ = {inner_x + scale(80), y, inner_right, y + target_height};
    y += target_height + scale(8);
    translation_label_rect_ = {inner_x, y, inner_right, y + label_height};
    y = translation_label_rect_.bottom + scale(8);
    translation_shell_rect_ = {inner_x, y, inner_right,
                               std::min(action_y - scale(8), y + translation_height)};

    const int action_button_width = scale(116);
    status_rect_ = {inner_x, action_y,
                    std::max(inner_x, inner_right - action_button_width - scale(8)),
                    action_y + action_height};
    MoveWindow(retranslate_, inner_right - action_button_width, action_y,
               action_button_width, action_height, TRUE);

    const RECT list_child = inset_rect(list_rect_, scale(1));
    MoveWindow(list_, list_child.left, list_child.top,
               std::max(1L, list_child.right - list_child.left),
               std::max(1L, list_child.bottom - list_child.top), TRUE);
    const RECT source_child = inset_rect(source_shell_rect_, scale(2));
    MoveWindow(source_, source_child.left, source_child.top,
               std::max(1L, source_child.right - source_child.left),
               std::max(1L, source_child.bottom - source_child.top), TRUE);
    MoveWindow(target_, target_combo_rect_.left, target_combo_rect_.top,
               std::max(1L, target_combo_rect_.right - target_combo_rect_.left),
               scale(220), TRUE);
    const RECT translation_child = inset_rect(translation_shell_rect_, scale(2));
    MoveWindow(translation_, translation_child.left, translation_child.top,
               std::max(1L, translation_child.right - translation_child.left),
               std::max(1L, translation_child.bottom - translation_child.top), TRUE);
    const auto set_edit_padding = [&](HWND edit) {
        RECT formatting{};
        GetClientRect(edit, &formatting);
        formatting.left += scale(7);
        formatting.right -= scale(7);
        formatting.top += scale(5);
        formatting.bottom -= scale(5);
        if (formatting.right > formatting.left && formatting.bottom > formatting.top) {
            SendMessageW(edit, EM_SETRECTNP, 0, reinterpret_cast<LPARAM>(&formatting));
        }
    };
    set_edit_padding(source_);
    set_edit_padding(translation_);

    const int close_width = scale(88);
    const int apply_width = scale(136);
    MoveWindow(close_, client.right - margin_x - close_width, footer_top,
               close_width, footer_height, TRUE);
    MoveWindow(apply_remaining_,
               client.right - margin_x - close_width - scale(8) - apply_width,
               footer_top, apply_width, footer_height, TRUE);
    refresh_list_hover();
    InvalidateRect(window_, nullptr, FALSE);
}

void BlockEditor::populate_blocks() {
    SendMessageW(list_, LB_RESETCONTENT, 0, 0);
    for (std::size_t index = 0; index < drafts_.size(); ++index) {
        const auto line = std::to_wstring(index + 1) + L"  " + preview(drafts_[index].source);
        SendMessageW(list_, LB_ADDSTRING, 0, reinterpret_cast<LPARAM>(line.c_str()));
    }
}

void BlockEditor::update_list_item(int index) {
    if (index < 0 || index >= static_cast<int>(drafts_.size())) return;
    if (list_tooltip_ && IsWindow(list_tooltip_)) {
        SendMessageW(list_tooltip_, TTM_POP, 0, 0);
    }
    list_tooltip_text_.clear();
    const int top_index = static_cast<int>(SendMessageW(list_, LB_GETTOPINDEX, 0, 0));
    const auto line = std::to_wstring(index + 1) + L"  " + preview(drafts_[index].source);
    SendMessageW(list_, LB_DELETESTRING, index, 0);
    SendMessageW(list_, LB_INSERTSTRING, index, reinterpret_cast<LPARAM>(line.c_str()));
    SendMessageW(list_, LB_SETCURSEL, current_, 0);
    if (top_index >= 0) SendMessageW(list_, LB_SETTOPINDEX, top_index, 0);
}

void BlockEditor::create_list_tooltip() {
    destroy_list_tooltip();
    if (!window_ || !list_) return;

    list_tooltip_ = CreateWindowExW(
        WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr,
        WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
        CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
        window_, nullptr, instance_, nullptr);
    if (!list_tooltip_) return;

    SetWindowTheme(list_tooltip_, L"DarkMode_Explorer", nullptr);
    SendMessageW(list_tooltip_, TTM_SETTIPBKCOLOR,
                 static_cast<WPARAM>(RGB(38, 40, 46)), 0);
    SendMessageW(list_tooltip_, TTM_SETTIPTEXTCOLOR,
                 static_cast<WPARAM>(RGB(242, 243, 245)), 0);
    SendMessageW(list_tooltip_, TTM_SETMAXTIPWIDTH, 0, scale(420));
    RECT margin{scale(6), scale(4), scale(6), scale(4)};
    SendMessageW(list_tooltip_, TTM_SETMARGIN, 0,
                 reinterpret_cast<LPARAM>(&margin));
    SetWindowPos(list_tooltip_, HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);

    TOOLINFOW tool{};
    tool.cbSize = TTTOOLINFOW_V2_SIZE;
    tool.uFlags = TTF_IDISHWND | TTF_SUBCLASS | TTF_TRANSPARENT;
    tool.hwnd = window_;
    tool.uId = reinterpret_cast<UINT_PTR>(list_);
    tool.lpszText = LPSTR_TEXTCALLBACKW;
    if (!SendMessageW(list_tooltip_, TTM_ADDTOOLW, 0,
                      reinterpret_cast<LPARAM>(&tool))) {
        DestroyWindow(list_tooltip_);
        list_tooltip_ = nullptr;
    }
}

void BlockEditor::destroy_list_tooltip() noexcept {
    if (list_tooltip_ && IsWindow(list_tooltip_)) {
        SendMessageW(list_tooltip_, TTM_POP, 0, 0);
        DestroyWindow(list_tooltip_);
    }
    list_tooltip_ = nullptr;
    list_tooltip_text_.clear();
}

void BlockEditor::refresh_list_hover() {
    int next = -1;
    if (list_ && IsWindow(list_)) {
        POINT point{};
        RECT client{};
        if (GetCursorPos(&point) && ScreenToClient(list_, &point) &&
            GetClientRect(list_, &client) && PtInRect(&client, point)) {
            const DWORD hit = static_cast<DWORD>(
                SendMessageW(list_, LB_ITEMFROMPOINT, 0, MAKELPARAM(point.x, point.y)));
            const int candidate = static_cast<int>(LOWORD(hit));
            if (HIWORD(hit) == 0 && candidate >= 0 &&
                candidate < static_cast<int>(drafts_.size())) {
                next = candidate;
            }
        }
    }
    set_list_hover(next);
}

void BlockEditor::set_list_hover(int index) {
    if (index == list_hover_) return;
    if (list_tooltip_ && IsWindow(list_tooltip_)) {
        SendMessageW(list_tooltip_, TTM_POP, 0, 0);
    }
    list_tooltip_text_.clear();

    const auto invalidate_item = [&](int item) {
        if (!list_ || !IsWindow(list_) || item < 0) return;
        RECT rect{};
        if (SendMessageW(list_, LB_GETITEMRECT, item,
                         reinterpret_cast<LPARAM>(&rect)) != LB_ERR) {
            InvalidateRect(list_, &rect, FALSE);
        }
    };
    const int previous = list_hover_;
    list_hover_ = index;
    invalidate_item(previous);
    invalidate_item(list_hover_);
}

bool BlockEditor::list_item_is_truncated(int index) const {
    if (!list_ || !IsWindow(list_) || index < 0 ||
        index >= static_cast<int>(drafts_.size())) {
        return false;
    }

    bool source_truncated = false;
    const auto source_preview = preview(
        drafts_[static_cast<std::size_t>(index)].source, 32, &source_truncated);
    if (source_truncated) return true;

    RECT item_rect{};
    if (SendMessageW(list_, LB_GETITEMRECT, index,
                     reinterpret_cast<LPARAM>(&item_rect)) == LB_ERR) {
        return false;
    }
    const int available_width = std::max(
        0L, item_rect.right - item_rect.left - 2L * scale(13));
    if (available_width <= 0) return true;

    const auto line = std::to_wstring(index + 1) + L"  " + source_preview;
    HDC dc = GetDC(list_);
    if (!dc) return false;
    const auto old_font = SelectObject(dc, font_);
    SIZE extent{};
    const BOOL measured = GetTextExtentPoint32W(
        dc, line.c_str(), static_cast<int>(line.size()), &extent);
    SelectObject(dc, old_font);
    ReleaseDC(list_, dc);
    return measured && extent.cx > available_width;
}

void BlockEditor::load_current(int index) {
    if (index < 0 || index >= static_cast<int>(drafts_.size())) return;
    current_ = index;
    const auto& draft = drafts_[static_cast<std::size_t>(index)];
    loading_ = true;
    SetWindowTextW(source_, draft.source.c_str());
    SetWindowTextW(translation_, draft.translation.c_str());
    int target_index = 0;
    for (std::size_t i = 0; i < targets.size(); ++i) {
        if (targets[i].first == draft.target_mode) {
            target_index = static_cast<int>(i + 1);
            break;
        }
    }
    SendMessageW(target_, CB_SETCURSEL, target_index, 0);
    loading_ = false;
    refresh_current_state();
}

void BlockEditor::source_changed() {
    if (loading_ || current_ < 0 || current_ >= static_cast<int>(drafts_.size())) return;
    auto& draft = drafts_[static_cast<std::size_t>(current_)];
    draft.source = control_text(source_);
    if (!trim(draft.source).empty()) draft.error.clear();
    draft.success = false;
    update_list_item(current_);
    refresh_current_state();
}

void BlockEditor::target_changed() {
    if (loading_ || current_ < 0 || current_ >= static_cast<int>(drafts_.size())) return;
    const int selected = static_cast<int>(SendMessageW(target_, CB_GETCURSEL, 0, 0));
    auto& draft = drafts_[static_cast<std::size_t>(current_)];
    draft.target_mode = selected <= 0 ? L"" : std::wstring(targets[static_cast<std::size_t>(selected - 1)].first);
    draft.error.clear();
    draft.success = false;
    refresh_current_state();
}

bool BlockEditor::current_source_empty() const {
    if (current_ < 0 || current_ >= static_cast<int>(drafts_.size())) return true;
    return trim(drafts_[static_cast<std::size_t>(current_)].source).empty();
}

void BlockEditor::refresh_current_state() {
    if (current_ < 0 || current_ >= static_cast<int>(drafts_.size())) return;
    const auto& draft = drafts_[static_cast<std::size_t>(current_)];
    block_heading_ = L"文本块 " + std::to_wstring(current_ + 1) + L" / " +
                     std::to_wstring(drafts_.size());

    const auto confidence = block_confidence(draft.block);
    if (!confidence) {
        confidence_text_ = L"当前 OCR 引擎不提供置信度";
        confidence_color_ = color_text_dim;
    } else {
        const int percent = static_cast<int>(std::lround(*confidence * 100.0));
        confidence_text_ = L"识别置信度 " + std::to_wstring(percent) + L"%";
        if (*confidence < 0.75) {
            confidence_text_ += L" · 建议重点校对";
            confidence_color_ = color_warning;
        } else {
            confidence_color_ = color_text_dim;
        }
    }

    EnableWindow(source_, draft.busy ? FALSE : TRUE);
    EnableWindow(target_, draft.busy ? FALSE : TRUE);
    EnableWindow(retranslate_, draft.busy ? FALSE : TRUE);

    if (draft.busy) {
        status_text_ = L"正在重译这一块…";
        status_color_ = color_text_dim;
    } else if (!draft.error.empty()) {
        status_text_ = draft.error;
        status_color_ = color_bad;
    } else if (draft.success && !draft.dirty()) {
        status_text_ = L"这一块已应用并完成重译";
        status_color_ = accent_color_;
    } else if (draft.dirty()) {
        status_text_ = L"有未应用更改";
        status_color_ = color_warning;
    } else if (draft.target_mode.empty()) {
        status_text_ = L"当前自动译为" + target_display_name(draft.actual_target);
        status_color_ = color_text_dim;
    } else {
        status_text_ = L"已强制指定目标语言";
        status_color_ = color_text_dim;
    }
    refresh_apply_button();
    InvalidateRect(window_, nullptr, FALSE);
    InvalidateRect(retranslate_, nullptr, FALSE);
}

void BlockEditor::refresh_apply_button() {
    const bool dirty = std::any_of(drafts_.begin(), drafts_.end(),
                                   [](const Draft& draft) { return draft.dirty(); });
    const bool busy = std::any_of(drafts_.begin(), drafts_.end(),
                                  [](const Draft& draft) { return draft.busy; });
    EnableWindow(apply_remaining_, dirty && !busy ? TRUE : FALSE);
    InvalidateRect(apply_remaining_, nullptr, FALSE);
}

bool BlockEditor::validate_drafts(bool current_only) {
    const int begin = current_only ? current_ : 0;
    const int end = current_only ? current_ + 1 : static_cast<int>(drafts_.size());
    for (int index = begin; index < end; ++index) {
        if (index < 0 || index >= static_cast<int>(drafts_.size())) continue;
        auto& draft = drafts_[static_cast<std::size_t>(index)];
        auto source = trim(draft.source);
        if (source.empty()) {
            draft.error = L"识别原文不能为空";
            SendMessageW(list_, LB_SETCURSEL, index, 0);
            load_current(index);
            SetFocus(source_);
            return false;
        }
        if (source != draft.source) {
            draft.source = std::move(source);
            update_list_item(index);
            if (index == current_) {
                loading_ = true;
                SetWindowTextW(source_, draft.source.c_str());
                loading_ = false;
            }
        }
    }
    return true;
}

BlockEditor::Change BlockEditor::make_change(std::size_t index) const {
    const auto& draft = drafts_.at(index);
    TextBlock block = draft.block;
    const auto recognized = block.recognized_text();
    block.edited_text = draft.source;
    block.has_edited_text = draft.source != recognized;
    block.forced_target_language = draft.target_mode;
    return Change{index, std::move(block)};
}

void BlockEditor::notify_request_cancelled() noexcept {
    if (close_notified_) return;
    close_notified_ = true;
    if (!cancel_request_callback_) return;
    try {
        cancel_request_callback_(*this);
    } catch (...) {
        // Closing the editor must not let an application callback cross WndProc.
    }
}

void BlockEditor::accept() {
    if (!validate_drafts(false)) return;
    if (std::any_of(drafts_.begin(), drafts_.end(),
                    [](const Draft& draft) { return draft.busy; })) {
        return;
    }
    std::vector<Change> output;
    for (std::size_t index = 0; index < drafts_.size(); ++index) {
        if (drafts_[index].dirty()) output.push_back(make_change(index));
    }
    complete(std::move(output));
}

void BlockEditor::accept_current() {
    if (!validate_drafts(true)) return;
    if (current_ < 0 || current_ >= static_cast<int>(drafts_.size())) return;
    auto& draft = drafts_[static_cast<std::size_t>(current_)];
    if (draft.busy) return;
    draft.error.clear();
    draft.success = false;
    draft.submitted_source = draft.source;
    draft.submitted_target_mode = draft.target_mode;
    draft.busy = true;
    refresh_current_state();
    refresh_apply_button();
    try {
        if (!retranslate_callback_) {
            set_error(static_cast<std::size_t>(current_), L"单块重译功能不可用");
            return;
        }
        retranslate_callback_(*this, make_change(static_cast<std::size_t>(current_)));
    } catch (const std::exception& error) {
        set_error(static_cast<std::size_t>(current_), utf8_to_wide(error.what()));
    } catch (...) {
        set_error(static_cast<std::size_t>(current_), L"无法启动单块重译");
    }
}

void BlockEditor::cancel() noexcept {
    complete(std::nullopt);
}

void BlockEditor::complete(std::optional<std::vector<Change>> changes) noexcept {
    if (finished_) return;
    finished_ = true;
    notify_request_cancelled();
    auto completion = std::move(completion_callback_);
    const HWND window = window_;
    if (window && IsWindow(window)) {
        ShowWindow(window, SW_HIDE);
        DestroyWindow(window);
    }
    if (completion) {
        try {
            completion(std::move(changes));
        } catch (...) {
            // A UI completion must not cross the Win32 window procedure.
        }
    }
    if (active_editor == this) active_editor = nullptr;
    delete this;
}

void BlockEditor::set_translation(std::size_t index,
                                  const BlockTranslation& translation) {
    if (index >= drafts_.size()) return;
    auto& draft = drafts_[index];
    if (!draft.busy || !draft.submitted_source || !draft.submitted_target_mode) return;

    draft.block = translation.block;
    draft.source = *draft.submitted_source;
    draft.target_mode = *draft.submitted_target_mode;
    draft.applied_source = draft.source;
    draft.applied_target_mode = draft.target_mode;
    draft.translation = translation.translated;
    draft.actual_target = translation.target_language;
    draft.submitted_source.reset();
    draft.submitted_target_mode.reset();
    draft.error.clear();
    draft.busy = false;
    draft.success = true;

    update_list_item(static_cast<int>(index));
    if (static_cast<int>(index) == current_) {
        loading_ = true;
        SetWindowTextW(source_, draft.source.c_str());
        SetWindowTextW(translation_, draft.translation.c_str());
        loading_ = false;
    }
    refresh_current_state();
    refresh_apply_button();
}

void BlockEditor::set_error(std::size_t index, std::wstring message) {
    if (index >= drafts_.size()) return;
    auto& draft = drafts_[index];
    draft.submitted_source.reset();
    draft.submitted_target_mode.reset();
    draft.error = std::move(message);
    draft.busy = false;
    draft.success = false;
    refresh_current_state();
    refresh_apply_button();
}

void BlockEditor::paint() {
    PAINTSTRUCT state{};
    HDC target_dc = BeginPaint(window_, &state);
    if (!target_dc) return;
    RECT client{};
    GetClientRect(window_, &client);
    HDC dc = CreateCompatibleDC(target_dc);
    HBITMAP bitmap = dc ? CreateCompatibleBitmap(target_dc, client.right, client.bottom) : nullptr;
    if (!dc || !bitmap) {
        FillRect(target_dc, &client, background_);
        if (bitmap) DeleteObject(bitmap);
        if (dc) DeleteDC(dc);
        EndPaint(window_, &state);
        return;
    }
    const auto old_bitmap = SelectObject(dc, bitmap);
    FillRect(dc, &client, background_);
    draw_text(dc, L"校对识别文字", title_rect_, title_font_, RGB(242, 243, 246),
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    fill_round_rect(dc, list_rect_, scale(6), color_card, color_line);
    fill_round_rect(dc, card_rect_, scale(8), color_card, color_line);
    draw_text(dc, block_heading_, block_title_rect_, block_title_font_, color_text,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    draw_text(dc, confidence_text_, confidence_rect_, small_font_, confidence_color_,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    draw_text(dc, L"识别原文", source_label_rect_, small_font_, color_text_dim,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    draw_text(dc, L"目标语言", target_label_rect_, small_font_, color_text_dim,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    draw_text(dc, L"当前译文", translation_label_rect_, small_font_, color_text_dim,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    const bool source_focused = GetFocus() == source_;
    fill_round_rect(dc, source_shell_rect_, scale(6), color_input,
                    source_focused ? accent_color_ : color_line_high);
    const bool translation_focused = GetFocus() == translation_;
    fill_round_rect(dc, translation_shell_rect_, scale(6), color_translation,
                    translation_focused ? accent_color_ : color_line_high);
    draw_text(dc, status_text_, status_rect_, small_font_, status_color_,
              DT_LEFT | DT_VCENTER | DT_WORDBREAK | DT_END_ELLIPSIS);

    BitBlt(target_dc, 0, 0, client.right, client.bottom, dc, 0, 0, SRCCOPY);
    SelectObject(dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(dc);
    EndPaint(window_, &state);
}

void BlockEditor::draw_combo(HWND control, HDC dc) {
    RECT rect{};
    GetClientRect(control, &rect);
    FillRect(dc, &rect, card_background_);
    const bool enabled = IsWindowEnabled(control) != FALSE;
    const bool focused = GetFocus() == control || IsChild(control, GetFocus());
    fill_round_rect(dc, rect, scale(6), color_input,
                    focused ? accent_color_ : color_line_high);
    RECT text_rect = rect;
    text_rect.left += scale(9);
    text_rect.right -= scale(32);
    const auto value = control_text(control);
    draw_text(dc, value, text_rect, font_, enabled ? color_text : color_text_faint,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    HPEN pen = CreatePen(PS_SOLID, std::max(1, scale(2)),
                         enabled ? color_text_dim : color_text_faint);
    if (pen) {
        const auto old_pen = SelectObject(dc, pen);
        const int x = rect.right - scale(16);
        const int y = (rect.top + rect.bottom) / 2;
        MoveToEx(dc, x - scale(4), y - scale(2), nullptr);
        LineTo(dc, x, y + scale(2));
        LineTo(dc, x + scale(4), y - scale(2));
        SelectObject(dc, old_pen);
        DeleteObject(pen);
    }
}

LRESULT BlockEditor::draw_item(const DRAWITEMSTRUCT& item) {
    HDC dc = item.hDC;
    RECT rect = item.rcItem;
    const bool disabled = (item.itemState & ODS_DISABLED) != 0;
    const bool selected = (item.itemState & ODS_SELECTED) != 0;

    if (item.CtlType == ODT_LISTBOX && item.CtlID == id_list) {
        FillRect(dc, &rect, card_background_);
        const bool hovered = item.itemID != static_cast<UINT>(-1) &&
                             static_cast<int>(item.itemID) == list_hover_;
        if (selected || hovered) {
            RECT highlight = rect;
            highlight.left += scale(5);
            highlight.right -= scale(5);
            highlight.top += scale(2);
            highlight.bottom -= scale(2);
            const COLORREF highlight_color = selected
                ? RGB(42, 48, 57)
                : RGB(35, 38, 45);
            fill_round_rect(dc, highlight, scale(5), highlight_color, highlight_color);
        }
        if (item.itemID != static_cast<UINT>(-1)) {
            const LRESULT length = SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID, 0);
            std::wstring value;
            if (length >= 0) {
                value.resize(static_cast<std::size_t>(length) + 1);
                SendMessageW(item.hwndItem, LB_GETTEXT, item.itemID,
                             reinterpret_cast<LPARAM>(value.data()));
                value.resize(static_cast<std::size_t>(length));
            }
            rect.left += scale(13);
            rect.right -= scale(13);
            draw_text(dc, value, rect, font_, selected ? RGB(255, 255, 255) : color_text,
                      DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        }
        return TRUE;
    }

    if (item.CtlType == ODT_COMBOBOX && item.CtlID == id_target) {
        const bool list_item = (item.itemState & ODS_COMBOBOXEDIT) == 0;
        const bool highlighted = list_item && selected;
        HBRUSH brush = CreateSolidBrush(highlighted ? accent_color_ : color_input);
        FillRect(dc, &rect, brush ? brush : input_background_);
        if (brush) DeleteObject(brush);
        std::wstring value;
        if (item.itemID != static_cast<UINT>(-1)) {
            const LRESULT length = SendMessageW(item.hwndItem, CB_GETLBTEXTLEN, item.itemID, 0);
            if (length >= 0) {
                value.resize(static_cast<std::size_t>(length) + 1);
                SendMessageW(item.hwndItem, CB_GETLBTEXT, item.itemID,
                             reinterpret_cast<LPARAM>(value.data()));
                value.resize(static_cast<std::size_t>(length));
            }
        }
        rect.left += scale(9);
        rect.right -= scale(9);
        draw_text(dc, value, rect, font_, disabled ? color_text_faint : color_text,
                  DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        return TRUE;
    }

    if (item.CtlType != ODT_BUTTON) return FALSE;
    const bool pressed = (item.itemState & ODS_SELECTED) != 0;
    const bool hot = (item.itemState & ODS_HOTLIGHT) != 0;
    const bool focused = (item.itemState & ODS_FOCUS) != 0;
    const bool primary = item.CtlID == id_retranslate || item.CtlID == id_apply_remaining;
    const bool inside_card = item.CtlID == id_retranslate;
    FillRect(dc, &rect, inside_card ? card_background_ : background_);
    COLORREF fill{};
    COLORREF border{};
    COLORREF foreground{};
    if (primary) {
        fill = disabled ? RGB(32, 35, 41)
                : pressed ? adjust_rgb(accent_color_, -12)
                : hot ? adjust_rgb(accent_color_, 18) : accent_color_;
        border = disabled ? RGB(44, 48, 56) : fill;
        foreground = disabled ? color_text_faint : RGB(14, 16, 19);
    } else {
        fill = pressed ? RGB(31, 34, 41) : hot ? RGB(31, 34, 41) : color_page;
        border = focused ? accent_color_ : color_line_high;
        foreground = disabled ? color_text_faint : color_text_dim;
    }
    fill_round_rect(dc, rect, scale(6), fill, border);
    draw_text(dc, control_text(item.hwndItem), rect,
              primary ? primary_font_ : font_, foreground,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    return TRUE;
}

LRESULT CALLBACK BlockEditor::combo_proc(HWND control, UINT message,
                                          WPARAM wparam, LPARAM lparam,
                                          UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<BlockEditor*>(reference);
    try {
        switch (message) {
        case WM_PAINT: {
            PAINTSTRUCT state{};
            HDC dc = BeginPaint(control, &state);
            if (dc && self) self->draw_combo(control, dc);
            EndPaint(control, &state);
            return 0;
        }
        case WM_PRINTCLIENT:
            if (self) self->draw_combo(control, reinterpret_cast<HDC>(wparam));
            return 0;
        case WM_ERASEBKGND:
            return 1;
        case WM_SETFOCUS:
        case WM_KILLFOCUS: {
            const LRESULT result = DefSubclassProc(control, message, wparam, lparam);
            if (self && self->window_) InvalidateRect(self->window_, nullptr, FALSE);
            InvalidateRect(control, nullptr, FALSE);
            return result;
        }
        case WM_NCDESTROY:
            RemoveWindowSubclass(control, &BlockEditor::combo_proc, subclass_id);
            break;
        default:
            break;
        }
    } catch (...) {
        if (message == WM_PAINT || message == WM_PRINTCLIENT) return 0;
    }
    return DefSubclassProc(control, message, wparam, lparam);
}

LRESULT CALLBACK BlockEditor::edit_proc(HWND control, UINT message,
                                         WPARAM wparam, LPARAM lparam,
                                         UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<BlockEditor*>(reference);
    switch (message) {
    case WM_SETFOCUS:
    case WM_KILLFOCUS: {
        const LRESULT result = DefSubclassProc(control, message, wparam, lparam);
        if (self && self->window_) InvalidateRect(self->window_, nullptr, FALSE);
        return result;
    }
    case WM_NCDESTROY:
        RemoveWindowSubclass(control, &BlockEditor::edit_proc, subclass_id);
        break;
    default:
        break;
    }
    return DefSubclassProc(control, message, wparam, lparam);
}

LRESULT CALLBACK BlockEditor::list_proc(HWND control, UINT message,
                                         WPARAM wparam, LPARAM lparam,
                                         UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<BlockEditor*>(reference);
    switch (message) {
    case WM_MOUSEMOVE:
        if (self) {
            if (!self->list_mouse_tracking_) {
                TRACKMOUSEEVENT tracking{};
                tracking.cbSize = sizeof(tracking);
                tracking.dwFlags = TME_LEAVE;
                tracking.hwndTrack = control;
                self->list_mouse_tracking_ = TrackMouseEvent(&tracking) != FALSE;
            }
            self->refresh_list_hover();
        }
        break;
    case WM_MOUSELEAVE:
        if (self) {
            self->list_mouse_tracking_ = false;
            self->set_list_hover(-1);
        }
        break;
    case WM_MOUSEWHEEL:
    case WM_VSCROLL:
    case WM_KEYDOWN:
    case WM_SIZE: {
        const LRESULT result = DefSubclassProc(control, message, wparam, lparam);
        if (self) self->refresh_list_hover();
        return result;
    }
    case WM_NCDESTROY: {
        if (self) {
            if (self->list_tooltip_ && IsWindow(self->list_tooltip_)) {
                SendMessageW(self->list_tooltip_, TTM_POP, 0, 0);
            }
            self->list_tooltip_text_.clear();
            self->list_hover_ = -1;
            self->list_mouse_tracking_ = false;
        }
        RemoveWindowSubclass(control, &BlockEditor::list_proc, subclass_id);
        const LRESULT result = DefSubclassProc(control, message, wparam, lparam);
        if (self && self->list_ == control) self->list_ = nullptr;
        return result;
    }
    default:
        break;
    }
    return DefSubclassProc(control, message, wparam, lparam);
}

void BlockEditor::present() {
    create_window();
    presented_ = true;
    ShowWindow(window_, SW_SHOW);
    SetWindowPos(window_, HWND_TOP, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    SetForegroundWindow(window_);
    SetFocus(list_);
}

LRESULT CALLBACK BlockEditor::window_proc(HWND window, UINT message,
                                           WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<BlockEditor*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
        self = static_cast<BlockEditor*>(create->lpCreateParams);
        self->window_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    if (!self) return DefWindowProcW(window, message, wparam, lparam);
    try {
        return self->handle_message(message, wparam, lparam);
    } catch (const std::exception& error) {
        self->window_error_ = error.what();
        if (message == WM_CREATE) return -1;
        if (self->window_) PostMessageW(self->window_, WM_CLOSE, 0, 0);
        return 0;
    } catch (...) {
        self->window_error_ = "unexpected block editor window error";
        if (message == WM_CREATE) return -1;
        if (self->window_) PostMessageW(self->window_, WM_CLOSE, 0, 0);
        return 0;
    }
}

LRESULT BlockEditor::handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_CREATE:
        create_controls();
        return 0;
    case WM_SIZE:
        layout_controls();
        return 0;
    case WM_GETMINMAXINFO: {
        auto* limits = reinterpret_cast<MINMAXINFO*>(lparam);
        RECT desired{0, 0, scale(590), scale(410)};
        AdjustWindowRectExForDpi(
            &desired, WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME,
            FALSE, WS_EX_TOOLWINDOW, static_cast<UINT>(dpi_));
        limits->ptMinTrackSize.x = desired.right - desired.left;
        limits->ptMinTrackSize.y = desired.bottom - desired.top;
        return 0;
    }
    case WM_DPICHANGED: {
        dpi_ = HIWORD(wparam);
        create_theme_resources();
        for (HWND control : {list_, source_, target_, translation_, retranslate_,
                             close_, apply_remaining_}) {
            set_font(control, (control == retranslate_ || control == apply_remaining_)
                                  ? primary_font_ : font_);
        }
        SendMessageW(list_, LB_SETITEMHEIGHT, 0, scale(34));
        SendMessageW(target_, CB_SETITEMHEIGHT, static_cast<WPARAM>(-1), scale(28));
        SendMessageW(target_, CB_SETITEMHEIGHT, 0, scale(28));
        if (list_tooltip_ && IsWindow(list_tooltip_)) {
            SendMessageW(list_tooltip_, TTM_POP, 0, 0);
            SendMessageW(list_tooltip_, TTM_SETMAXTIPWIDTH, 0, scale(420));
            RECT margin{scale(6), scale(4), scale(6), scale(4)};
            SendMessageW(list_tooltip_, TTM_SETMARGIN, 0,
                         reinterpret_cast<LPARAM>(&margin));
        }
        const auto* suggested = reinterpret_cast<RECT*>(lparam);
        SetWindowPos(window_, nullptr, suggested->left, suggested->top,
                     suggested->right - suggested->left,
                     suggested->bottom - suggested->top,
                     SWP_NOACTIVATE | SWP_NOZORDER);
        layout_controls();
        return 0;
    }
    case WM_MEASUREITEM: {
        auto* measure = reinterpret_cast<MEASUREITEMSTRUCT*>(lparam);
        if (measure->CtlID == id_list) {
            measure->itemHeight = static_cast<UINT>(scale(34));
            return TRUE;
        }
        if (measure->CtlID == id_target) {
            measure->itemHeight = static_cast<UINT>(scale(28));
            return TRUE;
        }
        break;
    }
    case WM_DRAWITEM:
        return draw_item(*reinterpret_cast<DRAWITEMSTRUCT*>(lparam));
    case WM_NOTIFY: {
        const auto* header = reinterpret_cast<NMHDR*>(lparam);
        if (header && header->hwndFrom == list_tooltip_ &&
            header->code == TTN_GETDISPINFOW) {
            auto* info = reinterpret_cast<NMTTDISPINFOW*>(lparam);
            list_tooltip_text_.clear();
            if (list_hover_ >= 0 &&
                list_hover_ < static_cast<int>(drafts_.size()) &&
                list_item_is_truncated(list_hover_)) {
                list_tooltip_text_ =
                    drafts_[static_cast<std::size_t>(list_hover_)].source;
            }
            info->lpszText = list_tooltip_text_.empty()
                ? const_cast<wchar_t*>(L"")
                : list_tooltip_text_.data();
            return 0;
        }
        break;
    }
    case WM_COMMAND: {
        const int identifier = LOWORD(wparam);
        const int notification = HIWORD(wparam);
        if (identifier == id_list && notification == LBN_SELCHANGE) {
            const int next = static_cast<int>(SendMessageW(list_, LB_GETCURSEL, 0, 0));
            if (next != current_) load_current(next);
            return 0;
        }
        if (identifier == id_source && notification == EN_CHANGE) {
            source_changed();
            return 0;
        }
        if (identifier == id_target && notification == CBN_SELCHANGE) {
            target_changed();
            InvalidateRect(target_, nullptr, FALSE);
            return 0;
        }
        if (identifier == id_retranslate && notification == BN_CLICKED) {
            accept_current();
            return 0;
        }
        if (identifier == id_apply_remaining && notification == BN_CLICKED) {
            accept();
            return 0;
        }
        if (identifier == IDCANCEL && notification == BN_CLICKED) {
            cancel();
            return 0;
        }
        break;
    }
    case WM_CTLCOLOREDIT: {
        const HWND child = reinterpret_cast<HWND>(lparam);
        HDC dc = reinterpret_cast<HDC>(wparam);
        SetTextColor(dc, color_text);
        SetBkColor(dc, child == translation_ ? color_translation : color_input);
        return reinterpret_cast<LRESULT>(child == translation_
                                             ? translation_background_
                                             : input_background_);
    }
    case WM_CTLCOLORSTATIC: {
        const HWND child = reinterpret_cast<HWND>(lparam);
        if (child == translation_) {
            HDC dc = reinterpret_cast<HDC>(wparam);
            SetTextColor(dc, RGB(201, 205, 212));
            SetBkColor(dc, color_translation);
            return reinterpret_cast<LRESULT>(translation_background_);
        }
        break;
    }
    case WM_CTLCOLORLISTBOX: {
        const HWND child = reinterpret_cast<HWND>(lparam);
        HDC dc = reinterpret_cast<HDC>(wparam);
        SetTextColor(dc, color_text);
        const bool block_list = child == list_;
        SetBkColor(dc, block_list ? color_card : color_input);
        return reinterpret_cast<LRESULT>(block_list ? card_background_ : input_background_);
    }
    case WM_KEYDOWN:
        if (wparam == VK_ESCAPE) {
            cancel();
            return 0;
        }
        break;
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT:
        paint();
        return 0;
    case WM_CLOSE:
        cancel();
        return 0;
    case WM_DESTROY:
        destroy_list_tooltip();
        notify_request_cancelled();
        return 0;
    case WM_NCDESTROY: {
        const HWND destroyed = window_;
        const bool abandoned = presented_ && !finished_;
        SetWindowLongPtrW(destroyed, GWLP_USERDATA, 0);
        window_ = nullptr;
        const LRESULT result = DefWindowProcW(destroyed, message, wparam, lparam);
        if (abandoned) {
            finished_ = true;
            notify_request_cancelled();
            auto completion = std::move(completion_callback_);
            if (completion) {
                try { completion(std::nullopt); } catch (...) {}
            }
            if (active_editor == this) active_editor = nullptr;
            delete this;
        }
        return result;
    }
    default:
        break;
    }
    return DefWindowProcW(window_, message, wparam, lparam);
}

}  // namespace screentrans
