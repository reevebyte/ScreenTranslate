#include "text_translation_window.hpp"

#include "language.hpp"
#include "resource.h"
#include "util.hpp"

#include <commctrl.h>
#include <dwmapi.h>
#include <psapi.h>
#include <tlhelp32.h>
#include <uxtheme.h>
#include <windowsx.h>

#include <algorithm>
#include <array>
#include <cstdlib>
#include <stdexcept>

namespace screentrans {

namespace {

constexpr wchar_t full_window_class[] = L"ScreenTranslate.Native.TextTranslateWindow.v1";
constexpr wchar_t quick_window_class[] = L"ScreenTranslate.Native.QuickTranslateWindow.v1";
constexpr UINT_PTR input_subclass_id = 11;
constexpr UINT_PTR target_combo_subclass_id = 12;

constexpr COLORREF color_background = RGB(21, 22, 27);
constexpr COLORREF color_surface = RGB(27, 29, 35);
constexpr COLORREF color_input = RGB(33, 36, 41);
constexpr COLORREF color_border = RGB(40, 43, 51);
constexpr COLORREF color_hover = RGB(45, 48, 56);
constexpr COLORREF color_text = RGB(231, 233, 236);
constexpr COLORREF color_dim = RGB(148, 154, 164);
constexpr COLORREF color_faint = RGB(110, 116, 126);
constexpr COLORREF color_error = RGB(255, 107, 107);

constexpr int id_target = 4101;
constexpr int id_input = 4102;
constexpr int id_output = 4103;
constexpr int id_clear = 4104;
constexpr int id_copy = 4105;
constexpr int id_settings = 4106;
constexpr int id_close = 4107;

struct ProcessResourceSnapshot {
    DWORD gdi_objects{};
    DWORD user_objects{};
    DWORD threads{};
    SIZE_T private_bytes{};
};

DWORD process_thread_count() noexcept {
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return 0;
    THREADENTRY32 entry{sizeof(entry)};
    DWORD count = 0;
    if (Thread32First(snapshot, &entry)) {
        const DWORD process = GetCurrentProcessId();
        do {
            if (entry.th32OwnerProcessID == process) ++count;
        } while (Thread32Next(snapshot, &entry));
    }
    CloseHandle(snapshot);
    return count;
}

SIZE_T process_private_bytes() noexcept {
    using GetMemoryInfo = BOOL(WINAPI*)(HANDLE, PPROCESS_MEMORY_COUNTERS, DWORD);
    const auto function = reinterpret_cast<GetMemoryInfo>(
        GetProcAddress(GetModuleHandleW(L"kernel32.dll"), "K32GetProcessMemoryInfo"));
    if (!function) return 0;
    PROCESS_MEMORY_COUNTERS_EX counters{};
    counters.cb = sizeof(counters);
    return function(GetCurrentProcess(),
                    reinterpret_cast<PPROCESS_MEMORY_COUNTERS>(&counters),
                    sizeof(counters))
        ? counters.PrivateUsage : 0;
}

ProcessResourceSnapshot process_resources() noexcept {
    return ProcessResourceSnapshot{
        GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
        GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
        process_thread_count(), process_private_bytes(),
    };
}

template <std::size_t Count>
bool resource_series_stable(
        const std::array<ProcessResourceSnapshot, Count>& samples) noexcept {
    static_assert(Count >= 5);
    constexpr DWORD total_gdi_tolerance = 8;
    constexpr DWORD total_user_tolerance = 4;
    constexpr DWORD total_thread_tolerance = 1;
    constexpr SIZE_T total_private_tolerance = 3U * 1024U * 1024U;
    constexpr SIZE_T per_batch_private_tolerance = 2U * 1024U * 1024U;

    constexpr std::size_t warmup_batches = 3;
    const auto& first = samples[warmup_batches];
    const auto& last = samples.back();
    if (last.gdi_objects > first.gdi_objects + total_gdi_tolerance ||
        last.user_objects > first.user_objects + total_user_tolerance ||
        (first.threads && last.threads > first.threads + total_thread_tolerance) ||
        (first.private_bytes &&
         last.private_bytes > first.private_bytes + total_private_tolerance)) {
        return false;
    }

    int significant_growth_streak = 0;
    int gdi_growth_streak = 0;
    int user_growth_streak = 0;
    int thread_growth_streak = 0;
    int private_growth_streak = 0;
    for (std::size_t index = 1; index < samples.size(); ++index) {
        const auto& before = samples[index - 1];
        const auto& after = samples[index];
        const bool significant_growth =
            after.gdi_objects > before.gdi_objects + 1 ||
            after.user_objects > before.user_objects + 1 ||
            (before.threads && after.threads > before.threads) ||
            (before.private_bytes &&
             after.private_bytes > before.private_bytes + per_batch_private_tolerance);
        significant_growth_streak = significant_growth
            ? significant_growth_streak + 1 : 0;
        if (significant_growth_streak >= 2) return false;

        gdi_growth_streak = after.gdi_objects > before.gdi_objects
            ? gdi_growth_streak + 1 : 0;
        user_growth_streak = after.user_objects > before.user_objects
            ? user_growth_streak + 1 : 0;
        thread_growth_streak = before.threads && after.threads > before.threads
            ? thread_growth_streak + 1 : 0;
        constexpr SIZE_T private_growth_floor = 128U * 1024U;
        private_growth_streak =
            before.private_bytes &&
            after.private_bytes > before.private_bytes + private_growth_floor
                ? private_growth_streak + 1 : 0;
        if (gdi_growth_streak >= 3 || user_growth_streak >= 3 ||
            thread_growth_streak >= 3 || private_growth_streak >= 3) {
            return false;
        }
    }
    return true;
}

std::string resource_change(const ProcessResourceSnapshot& before,
                            const ProcessResourceSnapshot& after) {
    return " gdi=" + std::to_string(before.gdi_objects) + "->" +
           std::to_string(after.gdi_objects) + " user=" +
           std::to_string(before.user_objects) + "->" +
           std::to_string(after.user_objects) + " threads=" +
           std::to_string(before.threads) + "->" + std::to_string(after.threads) +
           " private=" + std::to_string(before.private_bytes) + "->" +
           std::to_string(after.private_bytes);
}

void settle_self_test_ui() noexcept {
    DwmFlush();
    Sleep(500);
    MSG message{};
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    DwmFlush();
}

HMENU control_id(int id) noexcept {
    return reinterpret_cast<HMENU>(static_cast<INT_PTR>(id));
}

int scaled(int value, int dpi) noexcept {
    return MulDiv(value, dpi, 96);
}

std::wstring window_text(HWND control) {
    if (!control) return {};
    const int length = GetWindowTextLengthW(control);
    if (length <= 0) return {};
    std::wstring value(static_cast<std::size_t>(length) + 1, L'\0');
    GetWindowTextW(control, value.data(), length + 1);
    value.resize(static_cast<std::size_t>(length));
    return value;
}

void register_window_class(HINSTANCE instance, const wchar_t* name, WNDPROC procedure) {
    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.style = CS_HREDRAW | CS_VREDRAW;
    description.lpfnWndProc = procedure;
    description.hInstance = instance;
    description.hIcon = LoadIconW(instance, MAKEINTRESOURCEW(IDI_APP_ICON));
    description.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    description.lpszClassName = name;
    if (!RegisterClassExW(&description) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register text translation window");
    }
}

void apply_dark_frame(HWND window, bool quick) {
    const BOOL dark = TRUE;
    constexpr DWORD immersive_dark_mode = 20;
    DwmSetWindowAttribute(window, immersive_dark_mode, &dark, sizeof(dark));
    constexpr DWORD corner_preference = 33;
    const DWORD corner = quick ? 3U : 2U;
    DwmSetWindowAttribute(window, corner_preference, &corner, sizeof(corner));
    const COLORREF border = color_border;
    constexpr DWORD border_color = 34;
    DwmSetWindowAttribute(window, border_color, &border, sizeof(border));
    SetWindowTheme(window, L"DarkMode_Explorer", nullptr);
}

HFONT create_font(int dpi, int logical_pixels, int weight,
                  const std::wstring& family) {
    return CreateFontW(-scaled(logical_pixels, dpi), 0, 0, 0, weight, FALSE, FALSE,
                       FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                       CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE, family.c_str());
}

void set_control_font(HWND control, HFONT font) {
    if (control && font) SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
}

void add_tooltip(HWND owner, HWND control, const wchar_t* text) {
    HWND tooltip = CreateWindowExW(
        WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr,
        WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
        CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
        owner, nullptr, GetModuleHandleW(nullptr), nullptr);
    if (!tooltip) return;
    SetWindowTheme(tooltip, L"DarkMode_Explorer", nullptr);
    SendMessageW(tooltip, TTM_SETTIPBKCOLOR, static_cast<WPARAM>(color_input), 0);
    SendMessageW(tooltip, TTM_SETTIPTEXTCOLOR, static_cast<WPARAM>(color_text), 0);
    TOOLINFOW tool{};
    tool.cbSize = TTTOOLINFOW_V2_SIZE;
    tool.uFlags = TTF_IDISHWND | TTF_SUBCLASS | TTF_TRANSPARENT;
    tool.hwnd = owner;
    tool.uId = reinterpret_cast<UINT_PTR>(control);
    tool.lpszText = const_cast<LPWSTR>(text);
    if (!SendMessageW(tooltip, TTM_ADDTOOLW, 0, reinterpret_cast<LPARAM>(&tool))) {
        DestroyWindow(tooltip);
    }
}

void populate_targets(HWND combo) {
    SendMessageW(combo, CB_RESETCONTENT, 0, 0);
    SendMessageW(combo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(L"自动选择"));
    for (const auto target : text_translation_targets()) {
        const auto label = target_display_name(target);
        SendMessageW(combo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(label.c_str()));
    }
    SendMessageW(combo, CB_SETCURSEL, 0, 0);
}

int target_index(std::wstring_view target) {
    if (target.empty()) return 0;
    const auto& targets = text_translation_targets();
    const auto found = std::find(targets.begin(), targets.end(), target);
    return found == targets.end() ? 0 : static_cast<int>(std::distance(targets.begin(), found)) + 1;
}

std::wstring target_at(HWND combo) {
    const LRESULT selected = SendMessageW(combo, CB_GETCURSEL, 0, 0);
    if (selected <= 0) return {};
    const auto& targets = text_translation_targets();
    const auto index = static_cast<std::size_t>(selected - 1);
    return index < targets.size() ? std::wstring(targets[index]) : std::wstring{};
}

std::wstring session_status(const TextTranslationSession& session) {
    switch (session.state()) {
    case TextTranslationState::idle:
        return L"输入文字后，停止输入 500 毫秒自动翻译";
    case TextTranslationState::waiting:
        return L"等待输入完成…";
    case TextTranslationState::translating:
        return L"正在翻译为" + target_display_name(session.effective_target()) + L"…";
    case TextTranslationState::ready:
        return L"已翻译为" + target_display_name(session.effective_target());
    case TextTranslationState::error:
        return session.error().empty() ? L"文字翻译失败" : session.error();
    case TextTranslationState::too_long:
        return L"最多支持 5000 个字符，请删减后重试";
    }
    return {};
}

bool message_for(HWND window, const MSG& message) noexcept {
    return window && (message.hwnd == window || IsChild(window, message.hwnd));
}

void fill_rect(HDC dc, const RECT& rect, COLORREF color) {
    HBRUSH brush = CreateSolidBrush(color);
    FillRect(dc, &rect, brush);
    DeleteObject(brush);
}

void draw_rounded_control(HDC dc, RECT bounds, COLORREF fill, COLORREF border,
                          int radius) {
    HBRUSH brush = CreateSolidBrush(fill);
    HPEN pen = CreatePen(PS_SOLID, 1, border);
    const HGDIOBJ old_brush = SelectObject(dc, brush);
    const HGDIOBJ old_pen = SelectObject(dc, pen);
    RoundRect(dc, bounds.left, bounds.top, bounds.right, bounds.bottom, radius, radius);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
}

void draw_label(HDC dc, std::wstring_view text, RECT bounds, HFONT font,
                COLORREF color, UINT format) {
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, color);
    const HGDIOBJ previous = font ? SelectObject(dc, font) : nullptr;
    DrawTextW(dc, text.data(), static_cast<int>(text.size()), &bounds,
              format | DT_NOPREFIX);
    if (previous) SelectObject(dc, previous);
}

std::wstring target_selector_text(const TextTranslationSession& session,
                                  bool compact = false) {
    if (!session.target().empty()) return target_display_name(session.target());
    if (session.input().find_first_not_of(L" \t\r\n") == std::wstring::npos) {
        return L"自动选择";
    }
    auto target = target_display_name(session.effective_target());
    if (compact) {
        if (session.effective_target() == L"zh-Hans") target = L"中文";
        else if (session.effective_target() == L"zh-Hant") target = L"繁中";
    }
    return L"自动 · " + target;
}

void draw_target_selector(HWND combo, HDC dc, HFONT font, int dpi,
                          COLORREF accent, std::wstring_view value) {
    if (!combo || !dc) return;
    RECT bounds{};
    GetClientRect(combo, &bounds);
    fill_rect(dc, bounds, color_background);
    const bool focused = GetFocus() == combo;
    draw_rounded_control(dc, bounds, color_input, color_border,
                         scaled(7, dpi));
    if (focused) {
        HPEN focus_pen = CreatePen(PS_SOLID, std::max(1, scaled(2, dpi)), accent);
        const HGDIOBJ previous_focus_pen = SelectObject(dc, focus_pen);
        MoveToEx(dc, bounds.left + scaled(9, dpi),
                 bounds.bottom - scaled(2, dpi), nullptr);
        LineTo(dc, bounds.right - scaled(9, dpi),
               bounds.bottom - scaled(2, dpi));
        SelectObject(dc, previous_focus_pen);
        DeleteObject(focus_pen);
    }
    RECT text_bounds = bounds;
    text_bounds.left += scaled(11, dpi);
    text_bounds.right -= scaled(32, dpi);
    draw_label(dc, value, text_bounds, font, color_text,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

    HPEN pen = CreatePen(PS_SOLID, std::max(1, scaled(2, dpi)), color_dim);
    const HGDIOBJ previous = SelectObject(dc, pen);
    const int x = bounds.right - scaled(16, dpi);
    const int y = (bounds.top + bounds.bottom) / 2;
    MoveToEx(dc, x - scaled(4, dpi), y - scaled(2, dpi), nullptr);
    LineTo(dc, x, y + scaled(2, dpi));
    LineTo(dc, x + scaled(4, dpi), y - scaled(2, dpi));
    SelectObject(dc, previous);
    DeleteObject(pen);
}

void style_target_popup(HWND combo, int dpi, bool compact = false,
                        bool update_selection_height = true) {
    if (!combo) return;
    if (update_selection_height) {
        SendMessageW(combo, CB_SETITEMHEIGHT, static_cast<WPARAM>(-1),
                     scaled(compact ? 26 : 34, dpi));
    }
    SendMessageW(combo, CB_SETITEMHEIGHT, 0, scaled(32, dpi));
    SendMessageW(combo, CB_SETMINVISIBLE, 8, 0);
    COMBOBOXINFO info{sizeof(info)};
    if (!GetComboBoxInfo(combo, &info) || !info.hwndList) return;
    SetWindowTheme(info.hwndList, L"DarkMode_Explorer", nullptr);
    const BOOL dark = TRUE;
    constexpr DWORD immersive_dark_mode = 20;
    constexpr DWORD corner_preference = 33;
    const DWORD corner = 3;
    DwmSetWindowAttribute(info.hwndList, immersive_dark_mode, &dark, sizeof(dark));
    DwmSetWindowAttribute(info.hwndList, corner_preference, &corner, sizeof(corner));
    const LONG_PTR ex_style = GetWindowLongPtrW(info.hwndList, GWL_EXSTYLE);
    SetWindowLongPtrW(info.hwndList, GWL_EXSTYLE,
                      ex_style & ~static_cast<LONG_PTR>(WS_EX_CLIENTEDGE));
    SetWindowPos(info.hwndList, HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
}

bool point_in_window(HWND control, POINT screen_point) noexcept {
    RECT bounds{};
    return control && GetWindowRect(control, &bounds) &&
           PtInRect(&bounds, screen_point) != FALSE;
}

LRESULT draw_owner_item(const DRAWITEMSTRUCT& item, HFONT font, COLORREF accent,
                        HFONT icon_font = nullptr) {
    if (item.CtlType == ODT_BUTTON) {
        const bool selected = (item.itemState & ODS_SELECTED) != 0;
        const bool disabled = (item.itemState & ODS_DISABLED) != 0;
        const bool focused = (item.itemState & ODS_FOCUS) != 0;
        const bool hot = (item.itemState & ODS_HOTLIGHT) != 0;
        const int identifier = GetDlgCtrlID(item.hwndItem);
        const bool icon_button = icon_font &&
            (identifier == id_copy || identifier == id_settings ||
             identifier == id_close);
        if (icon_button) {
            SetDCBrushColor(item.hDC, color_background);
            FillRect(item.hDC, &item.rcItem,
                     reinterpret_cast<HBRUSH>(GetStockObject(DC_BRUSH)));
            const COLORREF fill = selected ? color_hover
                                  : hot ? color_surface : color_background;
            draw_rounded_control(item.hDC, item.rcItem, fill,
                                 focused ? accent : fill, 7);
            const wchar_t glyph = identifier == id_copy ? 0xE8C8
                                  : identifier == id_settings ? 0xE713
                                                              : 0xE711;
            draw_label(item.hDC, std::wstring_view(&glyph, 1), item.rcItem,
                       icon_font, disabled ? RGB(112, 112, 112)
                                           : hot || focused ? color_text : color_dim,
                       DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            return TRUE;
        }
        const COLORREF fill = selected ? color_hover : color_surface;
        draw_rounded_control(item.hDC, item.rcItem, fill,
                             focused ? accent : color_border, 8);
        wchar_t text[96]{};
        GetWindowTextW(item.hwndItem, text, static_cast<int>(std::size(text)));
        SetBkMode(item.hDC, TRANSPARENT);
        SetTextColor(item.hDC, disabled ? RGB(112, 112, 112) : color_text);
        const HGDIOBJ previous = font ? SelectObject(item.hDC, font) : nullptr;
        if (identifier == id_copy) {
            const int width = item.rcItem.right - item.rcItem.left;
            const int height = item.rcItem.bottom - item.rcItem.top;
            const int icon_width = std::clamp(height / 2, 12, 17);
            const int icon_height = std::clamp(height * 3 / 5, 14, 20);
            const int left = item.rcItem.left + (width - icon_width) / 2;
            const int top = item.rcItem.top + (height - icon_height) / 2;
            HPEN icon_pen = CreatePen(
                PS_SOLID, std::max(1, height / 24),
                disabled ? RGB(112, 112, 112) : color_text);
            const HGDIOBJ old_pen = SelectObject(item.hDC, icon_pen);
            const HGDIOBJ old_brush = SelectObject(item.hDC, GetStockObject(NULL_BRUSH));
            RoundRect(item.hDC, left + 4, top, left + icon_width, top + icon_height - 4,
                      3, 3);
            RoundRect(item.hDC, left, top + 4, left + icon_width - 4, top + icon_height,
                      3, 3);
            SelectObject(item.hDC, old_brush);
            SelectObject(item.hDC, old_pen);
            DeleteObject(icon_pen);
        } else {
            RECT label = item.rcItem;
            DrawTextW(item.hDC, text, -1, &label,
                      DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        }
        if (previous) SelectObject(item.hDC, previous);
        return TRUE;
    }
    if (item.CtlType == ODT_COMBOBOX) {
        RECT bounds = item.rcItem;
        fill_rect(item.hDC, bounds,
                  (item.itemState & ODS_SELECTED) ? color_hover : color_surface);
        std::wstring text;
        if (item.itemID != static_cast<UINT>(-1)) {
            const LRESULT length = SendMessageW(item.hwndItem, CB_GETLBTEXTLEN, item.itemID, 0);
            if (length >= 0) {
                text.resize(static_cast<std::size_t>(length) + 1);
                SendMessageW(item.hwndItem, CB_GETLBTEXT, item.itemID,
                             reinterpret_cast<LPARAM>(text.data()));
                text.resize(static_cast<std::size_t>(length));
            }
        }
        bounds.left += 10;
        bounds.right -= 8;
        SetBkMode(item.hDC, TRANSPARENT);
        SetTextColor(item.hDC,
                     (item.itemState & ODS_DISABLED) ? color_faint : color_text);
        const HGDIOBJ previous = font ? SelectObject(item.hDC, font) : nullptr;
        DrawTextW(item.hDC, text.c_str(), static_cast<int>(text.size()), &bounds,
                  DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
        if (previous) SelectObject(item.hDC, previous);
        return TRUE;
    }
    return FALSE;
}

void position_centered(HWND window, HWND owner, int logical_width, int logical_height) {
    HWND reference = owner && IsWindowVisible(owner) ? owner : GetForegroundWindow();
    HMONITOR monitor = MonitorFromWindow(reference, MONITOR_DEFAULTTONEAREST);
    MONITORINFO info{sizeof(info)};
    GetMonitorInfoW(monitor, &info);
    const int dpi = static_cast<int>(GetDpiForWindow(window));
    const int width = scaled(logical_width, dpi);
    const int height = scaled(logical_height, dpi);
    const int x = info.rcWork.left + ((info.rcWork.right - info.rcWork.left) - width) / 2;
    const int y = info.rcWork.top + ((info.rcWork.bottom - info.rcWork.top) - height) / 2;
    SetWindowPos(window, nullptr, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
}

void position_quick(HWND window) {
    HWND reference = GetForegroundWindow();
    HMONITOR monitor = MonitorFromWindow(reference, MONITOR_DEFAULTTONEAREST);
    MONITORINFO info{sizeof(info)};
    GetMonitorInfoW(monitor, &info);
    const int dpi = static_cast<int>(GetDpiForWindow(window));
    const int width = scaled(720, dpi);
    const int height = scaled(300, dpi);
    const int work_width = info.rcWork.right - info.rcWork.left;
    const int work_height = info.rcWork.bottom - info.rcWork.top;
    const int x = info.rcWork.left + (work_width - width) / 2;
    const int y = info.rcWork.top + std::max(scaled(36, dpi), (work_height - height) / 6);
    SetWindowPos(window, HWND_TOPMOST, x, y, width, height,
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

}  // namespace

TextTranslateWindow::TextTranslateWindow(HINSTANCE instance, ConfigStore& config,
                                         TextTranslationSession& session)
    : instance_(instance), config_(config), session_(session) {}

TextTranslateWindow::~TextTranslateWindow() {
    if (window_) DestroyWindow(window_);
    destroy_theme_resources();
}

bool TextTranslateWindow::visible() const noexcept {
    return window_ && IsWindowVisible(window_);
}

void TextTranslateWindow::create_window(HWND owner) {
    if (window_) return;
    register_window_class(instance_, full_window_class, &TextTranslateWindow::window_proc);
    window_ = CreateWindowExW(
        WS_EX_APPWINDOW, full_window_class, L"文字翻译", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
        CW_USEDEFAULT, CW_USEDEFAULT, 920, 600, nullptr, nullptr, instance_, this);
    if (!window_) throw_last_error("create text translation window");
    dpi_ = static_cast<int>(GetDpiForWindow(window_));
    apply_dark_frame(window_, false);
    create_theme_resources();
    create_controls();
    position_initial(owner);
    refresh();
}

void TextTranslateWindow::create_theme_resources() {
    destroy_theme_resources();
    accent_ = parse_rgb_color(config_.string(L"appearance.accent", L"#28C76F"));
    const auto family = config_.string(L"appearance.font_family", L"Microsoft YaHei UI");
    font_ = create_font(dpi_, 14, FW_NORMAL, family);
    title_font_ = create_font(dpi_, 15, FW_SEMIBOLD, family);
    small_font_ = create_font(dpi_, 12, FW_NORMAL, family);
    background_ = CreateSolidBrush(color_background);
    surface_ = CreateSolidBrush(color_surface);
    input_background_ = CreateSolidBrush(color_input);
}

void TextTranslateWindow::destroy_theme_resources() noexcept {
    if (font_) DeleteObject(font_);
    if (title_font_) DeleteObject(title_font_);
    if (small_font_) DeleteObject(small_font_);
    if (background_) DeleteObject(background_);
    if (surface_) DeleteObject(surface_);
    if (input_background_) DeleteObject(input_background_);
    font_ = title_font_ = small_font_ = nullptr;
    background_ = surface_ = input_background_ = nullptr;
}

void TextTranslateWindow::create_controls() {
    source_label_ = CreateWindowExW(0, L"STATIC", L"目标语言", WS_CHILD | WS_VISIBLE,
                                    0, 0, 0, 0, window_, nullptr, instance_, nullptr);
    target_combo_ = CreateWindowExW(0, L"COMBOBOX", L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL |
        CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS,
        0, 0, 0, 0, window_, control_id(id_target), instance_, nullptr);
    input_ = CreateWindowExW(0, L"EDIT", L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL |
        ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
        0, 0, 0, 0, window_, control_id(id_input), instance_, nullptr);
    output_ = CreateWindowExW(0, L"EDIT", L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL |
        ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY,
        0, 0, 0, 0, window_, control_id(id_output), instance_, nullptr);
    counter_ = CreateWindowExW(0, L"STATIC", L"0 / 5000", WS_CHILD | WS_VISIBLE,
                               0, 0, 0, 0, window_, nullptr, instance_, nullptr);
    status_ = CreateWindowExW(0, L"STATIC", L"", WS_CHILD | WS_VISIBLE | SS_LEFTNOWORDWRAP,
                              0, 0, 0, 0, window_, nullptr, instance_, nullptr);
    clear_button_ = CreateWindowExW(0, L"BUTTON", L"清空",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        0, 0, 0, 0, window_, control_id(id_clear), instance_, nullptr);
    copy_button_ = CreateWindowExW(0, L"BUTTON", L"复制",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        0, 0, 0, 0, window_, control_id(id_copy), instance_, nullptr);
    settings_button_ = CreateWindowExW(0, L"BUTTON", L"翻译设置",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        0, 0, 0, 0, window_, control_id(id_settings), instance_, nullptr);

    for (HWND control : {source_label_, target_combo_, input_, output_, counter_, status_,
                         clear_button_, copy_button_, settings_button_}) {
        SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
        set_control_font(control, font_);
    }
    set_control_font(source_label_, title_font_);
    set_control_font(counter_, small_font_);
    set_control_font(status_, small_font_);
    SendMessageW(input_, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN,
                 MAKELPARAM(scaled(12, dpi_), scaled(12, dpi_)));
    SendMessageW(output_, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN,
                 MAKELPARAM(scaled(12, dpi_), scaled(12, dpi_)));
    SendMessageW(input_, EM_SETCUEBANNER, TRUE,
                 reinterpret_cast<LPARAM>(L"输入或粘贴需要翻译的文字"));
    SendMessageW(output_, EM_SETCUEBANNER, TRUE,
                 reinterpret_cast<LPARAM>(L"译文会显示在这里"));
    SendMessageW(target_combo_, CB_SETITEMHEIGHT, static_cast<WPARAM>(-1), scaled(32, dpi_));
    SendMessageW(target_combo_, CB_SETITEMHEIGHT, 0, scaled(30, dpi_));
    populate_targets(target_combo_);
    SetWindowSubclass(target_combo_, &TextTranslateWindow::target_combo_proc,
                      target_combo_subclass_id,
                      reinterpret_cast<DWORD_PTR>(this));
    style_target_popup(target_combo_, dpi_);
    SetWindowSubclass(input_, &TextTranslateWindow::input_proc, input_subclass_id,
                      reinterpret_cast<DWORD_PTR>(this));
    add_tooltip(window_, copy_button_, L"复制译文");
    layout_controls();
}

void TextTranslateWindow::position_initial(HWND owner) {
    position_centered(window_, owner, 920, 600);
}

void TextTranslateWindow::layout_controls() {
    if (!window_) return;
    RECT client{};
    GetClientRect(window_, &client);
    const int width = client.right;
    const int height = client.bottom;
    const int margin = scaled(24, dpi_);
    const int header_height = scaled(34, dpi_);
    const int button_height = scaled(32, dpi_);
    const int gap = scaled(16, dpi_);
    const int footer_height = scaled(30, dpi_);
    const int combo_width = scaled(150, dpi_);
    const int clear_width = scaled(62, dpi_);
    const int copy_width = scaled(62, dpi_);
    const int settings_width = scaled(92, dpi_);

    MoveWindow(source_label_, margin, margin, scaled(96, dpi_), header_height, TRUE);
    MoveWindow(target_combo_, margin + scaled(104, dpi_), margin,
               combo_width, scaled(318, dpi_), TRUE);
    int right = width - margin;
    MoveWindow(settings_button_, right - settings_width, margin, settings_width,
               button_height, TRUE);
    right -= settings_width + scaled(8, dpi_);
    MoveWindow(copy_button_, right - copy_width, margin, copy_width, button_height, TRUE);
    right -= copy_width + scaled(8, dpi_);
    MoveWindow(clear_button_, right - clear_width, margin, clear_width, button_height, TRUE);

    const int body_top = margin + header_height + scaled(18, dpi_);
    const int body_bottom = height - margin - footer_height;
    const int body_height = std::max(scaled(180, dpi_), body_bottom - body_top);
    const int column_width = std::max(scaled(260, dpi_), (width - margin * 2 - gap) / 2);
    MoveWindow(input_, margin, body_top, column_width, body_height, TRUE);
    MoveWindow(output_, margin + column_width + gap, body_top,
               std::max(1, width - margin * 2 - gap - column_width), body_height, TRUE);
    MoveWindow(counter_, margin, height - margin - scaled(22, dpi_), scaled(110, dpi_),
               scaled(22, dpi_), TRUE);
    MoveWindow(status_, margin + scaled(120, dpi_), height - margin - scaled(22, dpi_),
               std::max(1, width - margin * 2 - scaled(120, dpi_)), scaled(22, dpi_), TRUE);
}

void TextTranslateWindow::sync_input() {
    if (window_text(input_) == session_.input()) return;
    syncing_ = true;
    SetWindowTextW(input_, session_.input().c_str());
    syncing_ = false;
}

void TextTranslateWindow::sync_target() {
    const int expected = target_index(session_.target());
    if (SendMessageW(target_combo_, CB_GETCURSEL, 0, 0) == expected) return;
    syncing_ = true;
    SendMessageW(target_combo_, CB_SETCURSEL, expected, 0);
    syncing_ = false;
}

void TextTranslateWindow::refresh() {
    if (!window_) return;
    sync_input();
    sync_target();
    if (window_text(output_) != session_.output()) {
        SetWindowTextW(output_, session_.output().c_str());
    }
    const auto counter = std::to_wstring(session_.character_count()) + L" / 5000";
    SetWindowTextW(counter_, counter.c_str());
    const auto status = session_status(session_);
    SetWindowTextW(status_, status.c_str());
    status_error_ = session_.state() == TextTranslationState::error ||
                    session_.state() == TextTranslationState::too_long;
    EnableWindow(copy_button_, !session_.output().empty());
    InvalidateRect(target_combo_, nullptr, FALSE);
    InvalidateRect(window_, nullptr, FALSE);
    InvalidateRect(status_, nullptr, TRUE);
}

void TextTranslateWindow::refresh_appearance() {
    if (!window_) return;
    create_theme_resources();
    for (HWND control : {source_label_, target_combo_, input_, output_, clear_button_,
                         copy_button_, settings_button_}) {
        set_control_font(control, control == source_label_ ? title_font_ : font_);
    }
    set_control_font(counter_, small_font_);
    set_control_font(status_, small_font_);
    apply_dark_frame(window_, false);
    InvalidateRect(window_, nullptr, TRUE);
}

void TextTranslateWindow::show(HWND owner) {
    create_window(owner);
    refresh();
    ShowWindow(window_, SW_SHOW);
    SetForegroundWindow(window_);
    SetFocus(input_);
}

void TextTranslateWindow::hide() {
    if (window_) ShowWindow(window_, SW_HIDE);
    if (callbacks_.composition_changed) callbacks_.composition_changed(false);
}

void TextTranslateWindow::on_input_changed() {
    if (!syncing_ && callbacks_.input_changed) callbacks_.input_changed(window_text(input_));
}

void TextTranslateWindow::on_target_changed() {
    if (!syncing_ && callbacks_.target_changed) callbacks_.target_changed(target_at(target_combo_));
}

LRESULT TextTranslateWindow::draw_item(const DRAWITEMSTRUCT& item) {
    return draw_owner_item(item, font_, accent_);
}

void TextTranslateWindow::draw_target_combo(HDC dc) {
    draw_target_selector(target_combo_, dc, font_, dpi_, accent_,
                         target_selector_text(session_));
}

void TextTranslateWindow::paint() {
    PAINTSTRUCT paint{};
    HDC dc = BeginPaint(window_, &paint);
    FillRect(dc, &paint.rcPaint, background_);
    EndPaint(window_, &paint);
}

bool TextTranslateWindow::preprocess_message(MSG& message) {
    if (!visible() || !message_for(window_, message)) return false;
    if (message.message == WM_KEYDOWN && message.wParam == VK_RETURN &&
        (GetKeyState(VK_CONTROL) & 0x8000)) {
        if (callbacks_.translate_now) callbacks_.translate_now();
        return true;
    }
    return IsDialogMessageW(window_, &message) != FALSE;
}

void TextTranslateWindow::self_test(HWND owner) {
    create_window(owner);
    if (!input_ || !output_ || !target_combo_ || !copy_button_ ||
        SendMessageW(target_combo_, CB_GETCOUNT, 0, 0) != 10) {
        throw AppError("text translation window self-test controls are incomplete");
    }
    const LONG_PTR style = GetWindowLongPtrW(window_, GWL_STYLE);
    if ((style & WS_THICKFRAME) == 0) {
        throw AppError("text translation window self-test is not resizable");
    }
    MINMAXINFO minimum{};
    SendMessageW(window_, WM_GETMINMAXINFO, 0, reinterpret_cast<LPARAM>(&minimum));
    if (minimum.ptMinTrackSize.x != scaled(720, dpi_) ||
        minimum.ptMinTrackSize.y != scaled(480, dpi_) ||
        dpi_ != static_cast<int>(GetDpiForWindow(window_))) {
        throw AppError("text translation window self-test size or DPI mismatch");
    }
    bool composition_started = false;
    bool composition_ended = false;
    auto composition_callback = callbacks_.composition_changed;
    callbacks_.composition_changed = [&](bool composing) {
        composition_started = composition_started || composing;
        composition_ended = composition_ended || !composing;
    };
    SendMessageW(input_, WM_IME_STARTCOMPOSITION, 0, 0);
    SendMessageW(input_, WM_IME_ENDCOMPOSITION, 0, 0);
    callbacks_.composition_changed = std::move(composition_callback);
    if (!composition_started || !composition_ended) {
        throw AppError("text translation window self-test IME handling mismatch");
    }
    show(owner);
    hide();
    for (int index = 0; index < 4; ++index) {
        show(owner);
        UpdateWindow(window_);
        UpdateWindow(target_combo_);
        hide();
    }
    settle_self_test_ui();
    std::array<ProcessResourceSnapshot, 9> resource_samples{};
    resource_samples[0] = process_resources();
    for (std::size_t batch = 1; batch < resource_samples.size(); ++batch) {
        for (int index = 0; index < 24; ++index) {
            show(owner);
            UpdateWindow(window_);
            UpdateWindow(target_combo_);
            hide();
        }
        resource_samples[batch] = process_resources();
    }
    if (!resource_series_stable(resource_samples)) {
        throw AppError("text translation window self-test leaked process resources" +
                       resource_change(resource_samples.front(),
                                       resource_samples.back()));
    }
    hide();
}

LRESULT CALLBACK TextTranslateWindow::input_proc(HWND control, UINT message,
                                                 WPARAM wparam, LPARAM lparam,
                                                 UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<TextTranslateWindow*>(reference);
    if (self && message == WM_IME_STARTCOMPOSITION && self->callbacks_.composition_changed) {
        self->callbacks_.composition_changed(true);
    } else if (self && message == WM_IME_ENDCOMPOSITION && self->callbacks_.composition_changed) {
        self->callbacks_.composition_changed(false);
    }
    if (self && (message == WM_SETFOCUS || message == WM_KILLFOCUS)) {
        InvalidateRect(self->window_, nullptr, FALSE);
    }
    if (message == WM_NCDESTROY) RemoveWindowSubclass(control, &TextTranslateWindow::input_proc,
                                                       subclass_id);
    return DefSubclassProc(control, message, wparam, lparam);
}

LRESULT CALLBACK TextTranslateWindow::target_combo_proc(
        HWND control, UINT message, WPARAM wparam, LPARAM lparam,
        UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<TextTranslateWindow*>(reference);
    switch (message) {
    case WM_PAINT: {
        PAINTSTRUCT paint{};
        HDC dc = BeginPaint(control, &paint);
        if (self) self->draw_target_combo(dc);
        EndPaint(control, &paint);
        return 0;
    }
    case WM_PRINTCLIENT:
        if (self) self->draw_target_combo(reinterpret_cast<HDC>(wparam));
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_SETFOCUS:
    case WM_KILLFOCUS: {
        const LRESULT result = DefSubclassProc(control, message, wparam, lparam);
        InvalidateRect(control, nullptr, FALSE);
        return result;
    }
    case WM_NCDESTROY:
        RemoveWindowSubclass(control, &TextTranslateWindow::target_combo_proc,
                             subclass_id);
        break;
    default:
        break;
    }
    return DefSubclassProc(control, message, wparam, lparam);
}

LRESULT CALLBACK TextTranslateWindow::window_proc(HWND window, UINT message,
                                                  WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<TextTranslateWindow*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<const CREATESTRUCTW*>(lparam);
        self = static_cast<TextTranslateWindow*>(create->lpCreateParams);
        self->window_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    if (!self) return DefWindowProcW(window, message, wparam, lparam);
    return self->handle_message(message, wparam, lparam);
}

LRESULT TextTranslateWindow::handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_COMMAND:
        switch (LOWORD(wparam)) {
        case id_input:
            if (HIWORD(wparam) == EN_CHANGE) on_input_changed();
            return 0;
        case id_target:
            if (HIWORD(wparam) == CBN_DROPDOWN) {
                style_target_popup(target_combo_, dpi_, false, false);
            }
            if (HIWORD(wparam) == CBN_SELCHANGE) on_target_changed();
            return 0;
        case id_clear:
            if (HIWORD(wparam) == BN_CLICKED && callbacks_.clear) callbacks_.clear();
            SetFocus(input_);
            return 0;
        case id_copy:
            if (HIWORD(wparam) == BN_CLICKED && callbacks_.copy) callbacks_.copy();
            return 0;
        case id_settings:
            if (HIWORD(wparam) == BN_CLICKED && callbacks_.open_settings) callbacks_.open_settings();
            return 0;
        default: break;
        }
        break;
    case WM_SIZE: layout_controls(); return 0;
    case WM_GETMINMAXINFO: {
        auto* info = reinterpret_cast<MINMAXINFO*>(lparam);
        info->ptMinTrackSize = POINT{scaled(720, dpi_), scaled(480, dpi_)};
        return 0;
    }
    case WM_DPICHANGED:
        dpi_ = HIWORD(wparam);
        if (const auto* suggested = reinterpret_cast<const RECT*>(lparam)) {
            SetWindowPos(window_, nullptr, suggested->left, suggested->top,
                         suggested->right - suggested->left,
                         suggested->bottom - suggested->top,
                         SWP_NOZORDER | SWP_NOACTIVATE);
        }
        refresh_appearance();
        layout_controls();
        return 0;
    case WM_DRAWITEM:
        if (lparam) return draw_item(*reinterpret_cast<const DRAWITEMSTRUCT*>(lparam));
        break;
    case WM_CTLCOLOREDIT: {
        HDC dc = reinterpret_cast<HDC>(wparam);
        SetTextColor(dc, color_text);
        SetBkColor(dc, color_input);
        return reinterpret_cast<LRESULT>(input_background_);
    }
    case WM_CTLCOLORSTATIC: {
        HDC dc = reinterpret_cast<HDC>(wparam);
        HWND control = reinterpret_cast<HWND>(lparam);
        SetTextColor(dc, control == status_ && status_error_ ? color_error :
                         control == counter_ || control == status_ ? color_dim : color_text);
        SetBkColor(dc, control == output_ ? color_input : color_background);
        return reinterpret_cast<LRESULT>(control == output_ ? input_background_ : background_);
    }
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: paint(); return 0;
    case WM_CLOSE: hide(); return 0;
    case WM_NCDESTROY:
        SetWindowLongPtrW(window_, GWLP_USERDATA, 0);
        window_ = nullptr;
        source_label_ = target_combo_ = input_ = output_ = counter_ = status_ = nullptr;
        clear_button_ = copy_button_ = settings_button_ = nullptr;
        return 0;
    default: break;
    }
    return DefWindowProcW(window_, message, wparam, lparam);
}

QuickTranslateWindow::QuickTranslateWindow(HINSTANCE instance, ConfigStore& config,
                                           TextTranslationSession& session)
    : instance_(instance), config_(config), session_(session) {}

QuickTranslateWindow::~QuickTranslateWindow() {
    if (window_) DestroyWindow(window_);
    destroy_theme_resources();
}

bool QuickTranslateWindow::visible() const noexcept {
    return window_ && IsWindowVisible(window_);
}

void QuickTranslateWindow::create_window(HWND owner) {
    if (window_) return;
    register_window_class(instance_, quick_window_class, &QuickTranslateWindow::window_proc);
    window_ = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW, quick_window_class, L"快速翻译",
        WS_POPUP | WS_CLIPCHILDREN, CW_USEDEFAULT, CW_USEDEFAULT, 720, 300,
        owner, nullptr, instance_, this);
    if (!window_) throw_last_error("create quick translation window");
    dpi_ = static_cast<int>(GetDpiForWindow(window_));
    apply_dark_frame(window_, true);
    create_theme_resources();
    create_controls();
    refresh();
}

void QuickTranslateWindow::create_theme_resources() {
    destroy_theme_resources();
    accent_ = parse_rgb_color(config_.string(L"appearance.accent", L"#28C76F"));
    const auto family = config_.string(L"appearance.font_family", L"Microsoft YaHei UI");
    font_ = create_font(dpi_, 14, FW_NORMAL, family);
    title_font_ = create_font(dpi_, 15, FW_SEMIBOLD, family);
    small_font_ = create_font(dpi_, 12, FW_NORMAL, family);
    icon_font_ = create_font(dpi_, 15, FW_NORMAL, L"Segoe Fluent Icons");
    background_ = CreateSolidBrush(color_background);
    surface_ = CreateSolidBrush(color_surface);
    input_background_ = CreateSolidBrush(color_surface);
}

void QuickTranslateWindow::destroy_theme_resources() noexcept {
    if (font_) DeleteObject(font_);
    if (title_font_) DeleteObject(title_font_);
    if (small_font_) DeleteObject(small_font_);
    if (icon_font_) DeleteObject(icon_font_);
    if (background_) DeleteObject(background_);
    if (surface_) DeleteObject(surface_);
    if (input_background_) DeleteObject(input_background_);
    font_ = title_font_ = small_font_ = icon_font_ = nullptr;
    background_ = surface_ = input_background_ = nullptr;
}

void QuickTranslateWindow::create_controls() {
    target_combo_ = CreateWindowExW(0, L"COMBOBOX", L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL |
        CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS,
        0, 0, 0, 0, window_, control_id(id_target), instance_, nullptr);
    input_ = CreateWindowExW(0, L"EDIT", L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP |
        ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN,
        0, 0, 0, 0, window_, control_id(id_input), instance_, nullptr);
    output_ = CreateWindowExW(0, L"EDIT", L"",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP |
        ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY,
        0, 0, 0, 0, window_, control_id(id_output), instance_, nullptr);
    copy_button_ = CreateWindowExW(0, L"BUTTON", L"复制",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        0, 0, 0, 0, window_, control_id(id_copy), instance_, nullptr);
    settings_button_ = CreateWindowExW(0, L"BUTTON", L"设置",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        0, 0, 0, 0, window_, control_id(id_settings), instance_, nullptr);
    close_button_ = CreateWindowExW(0, L"BUTTON", L"关闭",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
        0, 0, 0, 0, window_, control_id(id_close), instance_, nullptr);
    for (HWND control : {target_combo_, input_, output_, copy_button_, settings_button_,
                         close_button_}) {
        SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
        set_control_font(control, font_);
    }
    SendMessageW(input_, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN,
                 MAKELPARAM(scaled(12, dpi_), scaled(12, dpi_)));
    SendMessageW(output_, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN,
                 MAKELPARAM(scaled(12, dpi_), scaled(12, dpi_)));
    SendMessageW(input_, EM_SETCUEBANNER, TRUE,
                 reinterpret_cast<LPARAM>(L"输入文字，停顿后自动翻译"));
    SendMessageW(output_, EM_SETCUEBANNER, TRUE,
                 reinterpret_cast<LPARAM>(L"译文"));
    populate_targets(target_combo_);
    SetWindowSubclass(target_combo_, &QuickTranslateWindow::target_combo_proc,
                      target_combo_subclass_id,
                      reinterpret_cast<DWORD_PTR>(this));
    style_target_popup(target_combo_, dpi_, true);
    SetWindowSubclass(input_, &QuickTranslateWindow::input_proc, input_subclass_id,
                      reinterpret_cast<DWORD_PTR>(this));
    add_tooltip(window_, copy_button_, L"复制译文");
    add_tooltip(window_, settings_button_, L"打开翻译设置");
    add_tooltip(window_, close_button_, L"关闭快速翻译");
    layout_controls();
}

void QuickTranslateWindow::layout_controls() {
    if (!window_) return;
    RECT client{};
    GetClientRect(window_, &client);
    const int width = client.right;
    const int height = client.bottom;
    const int margin = scaled(16, dpi_);
    const int header_top = scaled(10, dpi_);
    const int control_height = scaled(30, dpi_);
    const int icon_width = scaled(30, dpi_);
    const int combo_width = scaled(132, dpi_);
    int right = width - margin;
    MoveWindow(close_button_, right - icon_width, header_top,
               icon_width, control_height, TRUE);
    right -= icon_width + scaled(6, dpi_);
    MoveWindow(settings_button_, right - icon_width, header_top,
               icon_width, control_height, TRUE);
    right -= icon_width + scaled(6, dpi_);
    MoveWindow(copy_button_, right - icon_width, header_top,
               icon_width, control_height, TRUE);
    right -= icon_width + scaled(8, dpi_);
    MoveWindow(target_combo_, right - combo_width, scaled(9, dpi_),
               combo_width, scaled(318, dpi_), TRUE);

    const int body_top = scaled(50, dpi_);
    const int body_bottom = height - scaled(38, dpi_);
    const int divider_y = body_top + scaled(88, dpi_);
    const int panel_inset = scaled(13, dpi_);
    const int label_height = scaled(29, dpi_);
    MoveWindow(input_, margin + panel_inset, body_top + label_height,
               width - margin * 2 - panel_inset * 2,
               std::max(scaled(42, dpi_),
                        divider_y - body_top - label_height - scaled(8, dpi_)), TRUE);
    MoveWindow(output_, margin + panel_inset, divider_y + label_height,
               width - margin * 2 - panel_inset * 2,
               std::max(scaled(56, dpi_),
                        body_bottom - divider_y - label_height - scaled(8, dpi_)), TRUE);
}

void QuickTranslateWindow::position_on_active_monitor() {
    position_quick(window_);
}

void QuickTranslateWindow::sync_input() {
    if (window_text(input_) == session_.input()) return;
    syncing_ = true;
    SetWindowTextW(input_, session_.input().c_str());
    syncing_ = false;
}

void QuickTranslateWindow::sync_target() {
    const int expected = target_index(session_.target());
    if (SendMessageW(target_combo_, CB_GETCURSEL, 0, 0) == expected) return;
    syncing_ = true;
    SendMessageW(target_combo_, CB_SETCURSEL, expected, 0);
    syncing_ = false;
}

void QuickTranslateWindow::refresh() {
    if (!window_) return;
    sync_input();
    sync_target();
    if (window_text(output_) != session_.output()) {
        SetWindowTextW(output_, session_.output().c_str());
    }
    status_error_ = session_.state() == TextTranslationState::error ||
                    session_.state() == TextTranslationState::too_long;
    EnableWindow(copy_button_, !session_.output().empty());
    InvalidateRect(target_combo_, nullptr, FALSE);
    InvalidateRect(window_, nullptr, FALSE);
}

void QuickTranslateWindow::refresh_appearance() {
    if (!window_) return;
    create_theme_resources();
    for (HWND control : {target_combo_, input_, output_, copy_button_, settings_button_,
                         close_button_}) {
        set_control_font(control, font_);
    }
    apply_dark_frame(window_, true);
    style_target_popup(target_combo_, dpi_, true);
    InvalidateRect(window_, nullptr, TRUE);
    InvalidateRect(target_combo_, nullptr, TRUE);
}

void QuickTranslateWindow::show(HWND owner) {
    create_window(owner);
    refresh();
    position_on_active_monitor();
    ShowWindow(window_, SW_SHOW);
    SetForegroundWindow(window_);
    SetFocus(input_);
    SendMessageW(input_, EM_SETSEL, 0, -1);
}

void QuickTranslateWindow::toggle(HWND owner) {
    if (visible()) hide();
    else show(owner);
}

void QuickTranslateWindow::hide() {
    if (window_) ShowWindow(window_, SW_HIDE);
    if (callbacks_.composition_changed) callbacks_.composition_changed(false);
}

void QuickTranslateWindow::on_input_changed() {
    if (!syncing_ && callbacks_.input_changed) callbacks_.input_changed(window_text(input_));
}

void QuickTranslateWindow::on_target_changed() {
    if (!syncing_ && callbacks_.target_changed) callbacks_.target_changed(target_at(target_combo_));
}

LRESULT QuickTranslateWindow::draw_item(const DRAWITEMSTRUCT& item) {
    if (item.CtlType == ODT_COMBOBOX && item.hwndItem == target_combo_ &&
        (item.itemState & ODS_COMBOBOXEDIT) != 0) {
        // Windows clears the closed selection field before this callback on
        // some focus transitions. Repaint that inner field with exactly the
        // same surface as the custom selector so text never disappears and no
        // square native selection layer becomes visible.
        fill_rect(item.hDC, item.rcItem, color_input);
        RECT text_bounds = item.rcItem;
        text_bounds.left += scaled(8, dpi_);
        text_bounds.right -= scaled(4, dpi_);
        draw_label(item.hDC, target_selector_text(session_, true), text_bounds,
                   small_font_, color_text,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        return TRUE;
    }
    return draw_owner_item(item, font_, accent_, icon_font_);
}

void QuickTranslateWindow::draw_target_combo(HDC dc) {
    draw_target_selector(target_combo_, dc, small_font_, dpi_, accent_,
                         target_selector_text(session_, true));
}

void QuickTranslateWindow::paint() {
    PAINTSTRUCT paint{};
    HDC dc = BeginPaint(window_, &paint);
    FillRect(dc, &paint.rcPaint, background_);
    RECT client{};
    GetClientRect(window_, &client);

    const int margin = scaled(16, dpi_);
    RECT mark{margin, scaled(12, dpi_), margin + scaled(26, dpi_), scaled(38, dpi_)};
    draw_rounded_control(dc, mark, accent_, accent_, scaled(6, dpi_));
    draw_label(dc, L"S", mark, title_font_, color_background,
               DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    draw_label(dc, L"快速翻译",
               RECT{mark.right + scaled(9, dpi_), scaled(9, dpi_),
                    mark.right + scaled(126, dpi_), scaled(41, dpi_)},
               title_font_, color_text,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    RECT body{margin, scaled(50, dpi_), client.right - margin,
              client.bottom - scaled(38, dpi_)};
    const int divider_y = body.top + scaled(88, dpi_);
    draw_rounded_control(dc, body, color_surface, color_border, scaled(8, dpi_));
    HPEN divider = CreatePen(PS_SOLID, 1, color_border);
    const HGDIOBJ previous_pen = SelectObject(dc, divider);
    MoveToEx(dc, body.left + scaled(13, dpi_), divider_y, nullptr);
    LineTo(dc, body.right - scaled(13, dpi_), divider_y);
    SelectObject(dc, previous_pen);
    DeleteObject(divider);

    const bool input_focused = GetFocus() == input_;
    const bool output_focused = GetFocus() == output_;
    draw_label(dc, L"原文",
               RECT{body.left + scaled(13, dpi_), body.top,
                    body.left + scaled(80, dpi_), body.top + scaled(29, dpi_)},
               small_font_, input_focused ? accent_ : color_dim,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    const auto counter = std::to_wstring(session_.character_count()) + L" / 5000";
    draw_label(dc, counter,
               RECT{body.right - scaled(112, dpi_), body.top,
                    body.right - scaled(13, dpi_), body.top + scaled(29, dpi_)},
               small_font_, session_.state() == TextTranslationState::too_long
                                ? color_error : color_faint,
               DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    draw_label(dc, L"译文",
               RECT{body.left + scaled(13, dpi_), divider_y,
                    body.left + scaled(80, dpi_), divider_y + scaled(29, dpi_)},
               small_font_, output_focused ? accent_ : color_dim,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    const auto status = session_status(session_);
    draw_label(dc, status,
               RECT{margin + scaled(2, dpi_), body.bottom + scaled(6, dpi_),
                    client.right - margin, client.bottom - scaled(8, dpi_)},
               small_font_, status_error_ ? color_error : color_dim,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    EndPaint(window_, &paint);
}

bool QuickTranslateWindow::preprocess_message(MSG& message) {
    if (!visible() || !message_for(window_, message)) return false;
    if (message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE) {
        hide();
        return true;
    }
    if (message.message == WM_KEYDOWN && message.wParam == VK_RETURN &&
        (GetKeyState(VK_CONTROL) & 0x8000)) {
        if (callbacks_.translate_now) callbacks_.translate_now();
        return true;
    }
    return IsDialogMessageW(window_, &message) != FALSE;
}

void QuickTranslateWindow::self_test(HWND owner) {
    create_window(owner);
    if (!input_ || !output_ || !target_combo_ || !copy_button_ ||
        !settings_button_ || !close_button_ ||
        SendMessageW(target_combo_, CB_GETCOUNT, 0, 0) != 10) {
        throw AppError("quick translation window self-test controls are incomplete");
    }
    const LONG_PTR ex_style = GetWindowLongPtrW(window_, GWL_EXSTYLE);
    if ((ex_style & WS_EX_TOOLWINDOW) == 0 || (ex_style & WS_EX_TOPMOST) == 0 ||
        (ex_style & WS_EX_APPWINDOW) != 0) {
        throw AppError("quick translation window self-test style is invalid");
    }
    if (dpi_ != static_cast<int>(GetDpiForWindow(window_))) {
        throw AppError("quick translation window self-test DPI mismatch");
    }
    RECT selector_client{};
    GetClientRect(target_combo_, &selector_client);
    HDC selector_dc = GetDC(target_combo_);
    HDC selector_test_dc = selector_dc ? CreateCompatibleDC(selector_dc) : nullptr;
    HBITMAP selector_bitmap = selector_dc
        ? CreateCompatibleBitmap(selector_dc,
                                 std::max(1L, selector_client.right),
                                 std::max(1L, selector_client.bottom))
        : nullptr;
    HGDIOBJ selector_previous = selector_test_dc && selector_bitmap
        ? SelectObject(selector_test_dc, selector_bitmap) : nullptr;
    bool closed_selector_painted = false;
    if (selector_test_dc && selector_previous) {
        RECT item_bounds = selector_client;
        item_bounds.right = std::max(item_bounds.left + 1,
                                     item_bounds.right - scaled(22, dpi_));
        DRAWITEMSTRUCT closed_item{};
        closed_item.CtlType = ODT_COMBOBOX;
        closed_item.CtlID = id_target;
        closed_item.itemID = 0;
        closed_item.itemAction = ODA_DRAWENTIRE;
        closed_item.itemState = ODS_COMBOBOXEDIT;
        closed_item.hwndItem = target_combo_;
        closed_item.hDC = selector_test_dc;
        closed_item.rcItem = item_bounds;
        draw_item(closed_item);
        closed_selector_painted =
            GetPixel(selector_test_dc, item_bounds.left, item_bounds.top) == color_input;
    }
    if (selector_previous) SelectObject(selector_test_dc, selector_previous);
    if (selector_bitmap) DeleteObject(selector_bitmap);
    if (selector_test_dc) DeleteDC(selector_test_dc);
    if (selector_dc) ReleaseDC(target_combo_, selector_dc);
    if (!closed_selector_painted) {
        throw AppError("quick translation window self-test closed target was not painted");
    }
    bool composition_started = false;
    bool composition_ended = false;
    auto composition_callback = callbacks_.composition_changed;
    callbacks_.composition_changed = [&](bool composing) {
        composition_started = composition_started || composing;
        composition_ended = composition_ended || !composing;
    };
    SendMessageW(input_, WM_IME_STARTCOMPOSITION, 0, 0);
    SendMessageW(input_, WM_IME_ENDCOMPOSITION, 0, 0);
    callbacks_.composition_changed = std::move(composition_callback);
    if (!composition_started || !composition_ended) {
        throw AppError("quick translation window self-test IME handling mismatch");
    }
    show(owner);
    RECT target_before_dropdown{};
    GetWindowRect(target_combo_, &target_before_dropdown);
    SendMessageW(target_combo_, CB_SHOWDROPDOWN, TRUE, 0);
    if (!SendMessageW(target_combo_, CB_GETDROPPEDSTATE, 0, 0)) {
        throw AppError("quick translation window self-test target menu did not open");
    }
    COMBOBOXINFO target_info{sizeof(target_info)};
    RECT target_list{};
    if (!GetComboBoxInfo(target_combo_, &target_info) || !target_info.hwndList ||
        !GetWindowRect(target_info.hwndList, &target_list) ||
        target_list.bottom - target_list.top < scaled(200, dpi_)) {
        throw AppError("quick translation window self-test target menu is clipped");
    }
    SendMessageW(target_combo_, CB_SHOWDROPDOWN, FALSE, 0);
    RECT target_after_dropdown{};
    if (!GetWindowRect(target_combo_, &target_after_dropdown) ||
        std::abs((target_after_dropdown.right - target_after_dropdown.left) -
                 (target_before_dropdown.right - target_before_dropdown.left)) > 1 ||
        std::abs((target_after_dropdown.bottom - target_after_dropdown.top) -
                 (target_before_dropdown.bottom - target_before_dropdown.top)) > 1 ||
        std::abs(target_after_dropdown.top - target_before_dropdown.top) > 1) {
        throw AppError("quick translation window self-test target selector resized");
    }
    SendMessageW(window_, WM_COMMAND, MAKEWPARAM(id_close, BN_CLICKED),
                 reinterpret_cast<LPARAM>(close_button_));
    if (visible()) {
        throw AppError("quick translation window self-test close button did not hide");
    }
    show(owner);
    POINT drag_point{scaled(240, dpi_), scaled(34, dpi_)};
    ClientToScreen(window_, &drag_point);
    const auto drag_hit = SendMessageW(
        window_, WM_NCHITTEST, 0,
        MAKELPARAM(static_cast<short>(drag_point.x),
                   static_cast<short>(drag_point.y)));
    RECT input_bounds{};
    GetWindowRect(input_, &input_bounds);
    const POINT input_point{
        (input_bounds.left + input_bounds.right) / 2,
        (input_bounds.top + input_bounds.bottom) / 2,
    };
    const auto input_hit = SendMessageW(
        window_, WM_NCHITTEST, 0,
        MAKELPARAM(static_cast<short>(input_point.x),
                   static_cast<short>(input_point.y)));
    if (drag_hit != HTCAPTION || input_hit != HTCLIENT) {
        throw AppError("quick translation window self-test drag regions mismatch");
    }
    hide();
    for (int index = 0; index < 4; ++index) {
        show(owner);
        UpdateWindow(window_);
        UpdateWindow(target_combo_);
        hide();
    }
    settle_self_test_ui();
    std::array<ProcessResourceSnapshot, 9> resource_samples{};
    resource_samples[0] = process_resources();
    for (std::size_t batch = 1; batch < resource_samples.size(); ++batch) {
        for (int index = 0; index < 24; ++index) {
            show(owner);
            UpdateWindow(window_);
            UpdateWindow(target_combo_);
            hide();
        }
        resource_samples[batch] = process_resources();
    }
    if (!resource_series_stable(resource_samples)) {
        throw AppError("quick translation window self-test leaked process resources" +
                       resource_change(resource_samples.front(),
                                       resource_samples.back()));
    }
    hide();
}

LRESULT CALLBACK QuickTranslateWindow::input_proc(HWND control, UINT message,
                                                  WPARAM wparam, LPARAM lparam,
                                                  UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<QuickTranslateWindow*>(reference);
    if (self && message == WM_IME_STARTCOMPOSITION && self->callbacks_.composition_changed) {
        self->callbacks_.composition_changed(true);
    } else if (self && message == WM_IME_ENDCOMPOSITION && self->callbacks_.composition_changed) {
        self->callbacks_.composition_changed(false);
    }
    if (self && (message == WM_SETFOCUS || message == WM_KILLFOCUS)) {
        InvalidateRect(self->window_, nullptr, FALSE);
    }
    if (message == WM_NCDESTROY) RemoveWindowSubclass(control, &QuickTranslateWindow::input_proc,
                                                       subclass_id);
    return DefSubclassProc(control, message, wparam, lparam);
}

LRESULT CALLBACK QuickTranslateWindow::target_combo_proc(
        HWND control, UINT message, WPARAM wparam, LPARAM lparam,
        UINT_PTR subclass_id, DWORD_PTR reference) {
    auto* self = reinterpret_cast<QuickTranslateWindow*>(reference);
    switch (message) {
    case WM_PAINT: {
        PAINTSTRUCT paint{};
        HDC dc = BeginPaint(control, &paint);
        if (self) self->draw_target_combo(dc);
        EndPaint(control, &paint);
        return 0;
    }
    case WM_PRINTCLIENT:
        if (self) self->draw_target_combo(reinterpret_cast<HDC>(wparam));
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_SETFOCUS:
    case WM_KILLFOCUS: {
        const LRESULT result = DefSubclassProc(control, message, wparam, lparam);
        RedrawWindow(control, nullptr, nullptr,
                     RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE);
        return result;
    }
    case WM_NCDESTROY:
        RemoveWindowSubclass(control, &QuickTranslateWindow::target_combo_proc,
                             subclass_id);
        break;
    default:
        break;
    }
    return DefSubclassProc(control, message, wparam, lparam);
}

LRESULT CALLBACK QuickTranslateWindow::window_proc(HWND window, UINT message,
                                                   WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<QuickTranslateWindow*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<const CREATESTRUCTW*>(lparam);
        self = static_cast<QuickTranslateWindow*>(create->lpCreateParams);
        self->window_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    if (!self) return DefWindowProcW(window, message, wparam, lparam);
    return self->handle_message(message, wparam, lparam);
}

LRESULT QuickTranslateWindow::handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_COMMAND:
        switch (LOWORD(wparam)) {
        case id_input:
            if (HIWORD(wparam) == EN_CHANGE) on_input_changed();
            if (HIWORD(wparam) == EN_SETFOCUS || HIWORD(wparam) == EN_KILLFOCUS) {
                InvalidateRect(window_, nullptr, FALSE);
            }
            return 0;
        case id_output:
            if (HIWORD(wparam) == EN_SETFOCUS || HIWORD(wparam) == EN_KILLFOCUS) {
                InvalidateRect(window_, nullptr, FALSE);
            }
            return 0;
        case id_target:
            if (HIWORD(wparam) == CBN_DROPDOWN) {
                style_target_popup(target_combo_, dpi_, true, false);
            }
            if (HIWORD(wparam) == CBN_SELCHANGE) on_target_changed();
            return 0;
        case id_copy:
            if (HIWORD(wparam) == BN_CLICKED && callbacks_.copy) callbacks_.copy();
            return 0;
        case id_settings:
            if (HIWORD(wparam) == BN_CLICKED && callbacks_.open_settings) callbacks_.open_settings();
            return 0;
        case id_close:
            if (HIWORD(wparam) == BN_CLICKED) hide();
            return 0;
        default: break;
        }
        break;
    case WM_SIZE: layout_controls(); return 0;
    case WM_DPICHANGED:
        dpi_ = HIWORD(wparam);
        if (const auto* suggested = reinterpret_cast<const RECT*>(lparam)) {
            SetWindowPos(window_, HWND_TOPMOST, suggested->left, suggested->top,
                         suggested->right - suggested->left,
                         suggested->bottom - suggested->top,
                         SWP_NOACTIVATE);
        }
        refresh_appearance();
        layout_controls();
        return 0;
    case WM_NCHITTEST: {
        const LRESULT hit = DefWindowProcW(window_, message, wparam, lparam);
        if (hit != HTCLIENT) return hit;
        const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        if (point_in_window(input_, point) || point_in_window(output_, point) ||
            point_in_window(target_combo_, point) || point_in_window(copy_button_, point) ||
            point_in_window(settings_button_, point) ||
            point_in_window(close_button_, point)) {
            return HTCLIENT;
        }
        return HTCAPTION;
    }
    case WM_DRAWITEM:
        if (lparam) return draw_item(*reinterpret_cast<const DRAWITEMSTRUCT*>(lparam));
        break;
    case WM_CTLCOLOREDIT: {
        HDC dc = reinterpret_cast<HDC>(wparam);
        SetTextColor(dc, color_text);
        SetBkColor(dc, color_surface);
        return reinterpret_cast<LRESULT>(input_background_);
    }
    case WM_CTLCOLORSTATIC: {
        HDC dc = reinterpret_cast<HDC>(wparam);
        SetTextColor(dc, color_text);
        SetBkColor(dc, color_surface);
        return reinterpret_cast<LRESULT>(input_background_);
    }
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: paint(); return 0;
    case WM_CLOSE: hide(); return 0;
    case WM_NCDESTROY:
        SetWindowLongPtrW(window_, GWLP_USERDATA, 0);
        window_ = nullptr;
        target_combo_ = input_ = output_ = nullptr;
        copy_button_ = settings_button_ = close_button_ = nullptr;
        return 0;
    default: break;
    }
    return DefWindowProcW(window_, message, wparam, lparam);
}

}  // namespace screentrans
