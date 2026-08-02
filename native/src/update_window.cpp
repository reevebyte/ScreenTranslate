#include "update_window.hpp"

#include "resource.h"
#include "util.hpp"
#include "version.hpp"

#include <commctrl.h>
#include <dwmapi.h>
#include <shellapi.h>
#include <shellscalingapi.h>
#include <uxtheme.h>

#include <algorithm>
#include <array>
#include <cwchar>
#include <memory>
#include <mutex>

namespace screentrans {

namespace {

constexpr wchar_t class_name[] = L"ScreenTranslate.Native.UpdateWindow.v2";
constexpr int id_check = 8101;
constexpr int id_release = 8102;
constexpr int id_action = 8103;

constexpr COLORREF color_page = RGB(21, 22, 27);
constexpr COLORREF color_card = RGB(27, 29, 35);
constexpr COLORREF color_input = RGB(33, 36, 41);
constexpr COLORREF color_line = RGB(40, 43, 51);
constexpr COLORREF color_line_high = RGB(52, 56, 66);
constexpr COLORREF color_text = RGB(231, 233, 236);
constexpr COLORREF color_text_dim = RGB(148, 154, 164);
constexpr COLORREF color_text_faint = RGB(110, 116, 126);
constexpr COLORREF color_ok = RGB(78, 209, 139);
constexpr COLORREF color_warn = RGB(232, 180, 74);
constexpr COLORREF color_bad = RGB(255, 107, 107);

int px(int value, int dpi) noexcept {
    return MulDiv(value, dpi, 96);
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

std::wstring text_of(HWND control) {
    const int length = GetWindowTextLengthW(control);
    std::wstring value(static_cast<std::size_t>(std::max(0, length)) + 1, L'\0');
    if (length > 0) GetWindowTextW(control, value.data(), length + 1);
    value.resize(static_cast<std::size_t>(std::max(0, length)));
    return value;
}

void set_font(HWND control, HFONT font) {
    if (control) SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
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
    const COLORREF text = color_text;
    DwmSetWindowAttribute(window, border_color, &border, sizeof(border));
    DwmSetWindowAttribute(window, caption_color, &caption, sizeof(caption));
    DwmSetWindowAttribute(window, caption_text_color, &text, sizeof(text));
}

std::wstring format_size(std::uint64_t bytes) {
    constexpr std::array<std::wstring_view, 7> units{
        L"B", L"KB", L"MB", L"GB", L"TB", L"PB", L"EB",
    };
    double value = static_cast<double>(bytes);
    std::size_t unit = 0;
    while (value >= 1024.0 && unit + 1 < units.size()) {
        value /= 1024.0;
        ++unit;
    }
    wchar_t buffer[64]{};
    if (unit == 0) {
        swprintf_s(buffer, L"%llu B", static_cast<unsigned long long>(bytes));
    } else {
        swprintf_s(buffer, L"%.1f %s", value, units[unit].data());
    }
    return buffer;
}

std::wstring format_published_at(std::wstring_view value) {
    if (value.empty()) return {};
    int year = 0;
    int month = 0;
    int day = 0;
    int hour = 0;
    int minute = 0;
    int second = 0;
    const std::wstring input(value);
    if (swscanf_s(input.c_str(), L"%d-%d-%dT%d:%d:%d",
                  &year, &month, &day, &hour, &minute, &second) != 6) {
        return input;
    }
    SYSTEMTIME parsed{};
    parsed.wYear = static_cast<WORD>(year);
    parsed.wMonth = static_cast<WORD>(month);
    parsed.wDay = static_cast<WORD>(day);
    parsed.wHour = static_cast<WORD>(hour);
    parsed.wMinute = static_cast<WORD>(minute);
    parsed.wSecond = static_cast<WORD>(second);

    const auto time_separator = input.find(L'T');
    const auto zone_position = input.find_last_of(L"Zz+-");
    const bool has_zone = zone_position != std::wstring::npos &&
                          time_separator != std::wstring::npos &&
                          zone_position > time_separator;
    SYSTEMTIME display = parsed;
    if (has_zone) {
        SYSTEMTIME utc = parsed;
        const wchar_t zone = input[zone_position];
        if (zone == L'+' || zone == L'-') {
            int zone_hour = 0;
            int zone_minute = 0;
            if (swscanf_s(input.c_str() + zone_position + 1, L"%d:%d",
                          &zone_hour, &zone_minute) != 2) {
                return input;
            }
            FILETIME file_time{};
            if (!SystemTimeToFileTime(&parsed, &file_time)) return input;
            ULARGE_INTEGER ticks{};
            ticks.LowPart = file_time.dwLowDateTime;
            ticks.HighPart = file_time.dwHighDateTime;
            const ULONGLONG offset = static_cast<ULONGLONG>(zone_hour * 60 + zone_minute) *
                                     60ULL * 10'000'000ULL;
            if (zone == L'+') {
                if (ticks.QuadPart < offset) return input;
                ticks.QuadPart -= offset;
            } else {
                ticks.QuadPart += offset;
            }
            file_time.dwLowDateTime = ticks.LowPart;
            file_time.dwHighDateTime = ticks.HighPart;
            if (!FileTimeToSystemTime(&file_time, &utc)) return input;
        }
        if (!SystemTimeToTzSpecificLocalTime(nullptr, &utc, &display)) return input;
    }

    wchar_t buffer[80]{};
    swprintf_s(buffer, has_zone ? L"%04u-%02u-%02u %02u:%02u（本地时间）"
                                : L"%04u-%02u-%02u %02u:%02u",
               display.wYear, display.wMonth, display.wDay,
               display.wHour, display.wMinute);
    return buffer;
}

std::wstring source_label(std::wstring_view repository) {
    if (repository.empty()) return L"本地开发构建";
    constexpr std::wstring_view prefix = L"https://github.com/";
    if (repository.starts_with(prefix)) {
        const auto owner_start = prefix.size();
        const auto slash = repository.find(L'/', owner_start);
        const auto owner = repository.substr(owner_start, slash - owner_start);
        if (!owner.empty()) return L"GitHub · " + std::wstring(owner);
    }
    return L"GitHub Release";
}

class InstallConfirmationDialog {
public:
    bool show(HINSTANCE instance, HWND owner, std::wstring version,
              COLORREF accent) {
        instance_ = instance;
        owner_ = owner;
        accent_ = accent;
        prompt_ =
            L"安装包已通过大小和 SHA-256 校验。\r\n\r\n"
            L"ScreenTranslate 将退出并启动 " + std::move(version) +
            L" 安装程序。\r\n"
            L"由于安装包未使用代码签名，Windows 可能显示“未知发布者”。\r\n\r\n"
            L"是否继续？";

        WNDCLASSEXW description{};
        description.cbSize = sizeof(description);
        description.lpfnWndProc = &InstallConfirmationDialog::window_proc;
        description.style = CS_HREDRAW | CS_VREDRAW;
        description.hInstance = instance_;
        description.hCursor = LoadCursorW(nullptr, IDC_ARROW);
        description.hIcon = LoadIconW(instance_, MAKEINTRESOURCEW(IDI_APP_ICON));
        description.hIconSm = description.hIcon;
        description.lpszClassName = window_class_name;
        if (!RegisterClassExW(&description) &&
            GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
            return false;
        }

        RECT anchor{};
        const bool has_owner_rect = owner_ && IsWindow(owner_) &&
                                    GetWindowRect(owner_, &anchor);
        HMONITOR monitor = has_owner_rect
            ? MonitorFromWindow(owner_, MONITOR_DEFAULTTONEAREST)
            : MonitorFromPoint(POINT{}, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO monitor_info{sizeof(monitor_info)};
        if (!GetMonitorInfoW(monitor, &monitor_info)) {
            monitor_info.rcWork = {0, 0, GetSystemMetrics(SM_CXSCREEN),
                                   GetSystemMetrics(SM_CYSCREEN)};
        }
        if (!has_owner_rect) anchor = monitor_info.rcWork;

        dpi_ = owner_ && IsWindow(owner_)
            ? static_cast<int>(GetDpiForWindow(owner_))
            : 0;
        if (dpi_ <= 0) {
            UINT dpi_x = 96;
            UINT dpi_y = 96;
            if (FAILED(GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI,
                                        &dpi_x, &dpi_y))) {
                dpi_x = GetDpiForSystem();
            }
            dpi_ = static_cast<int>(dpi_x ? dpi_x : 96);
        }
        if (!create_theme_resources()) return false;

        const DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        constexpr DWORD ex_style = WS_EX_DLGMODALFRAME;
        RECT desired{0, 0, px(520, dpi_), px(332, dpi_)};
        AdjustWindowRectExForDpi(&desired, style, FALSE, ex_style,
                                 static_cast<UINT>(dpi_));
        const int width = desired.right - desired.left;
        const int height = desired.bottom - desired.top;
        const LONG centered_x = anchor.left +
            ((anchor.right - anchor.left) - width) / 2;
        const LONG centered_y = anchor.top +
            ((anchor.bottom - anchor.top) - height) / 2;
        const LONG x = std::clamp(centered_x, monitor_info.rcWork.left,
                                  std::max(monitor_info.rcWork.left,
                                           monitor_info.rcWork.right - width));
        const LONG y = std::clamp(centered_y, monitor_info.rcWork.top,
                                  std::max(monitor_info.rcWork.top,
                                           monitor_info.rcWork.bottom - height));

        window_ = CreateWindowExW(
            ex_style, window_class_name, L"安装更新", style,
            x, y, width, height, owner_, nullptr, instance_, this);
        if (!window_) {
            destroy_theme_resources();
            return false;
        }
        enable_dark_frame(window_);

        disabled_owner_ = owner_ && IsWindow(owner_) && IsWindowEnabled(owner_);
        if (disabled_owner_) EnableWindow(owner_, FALSE);
        ShowWindow(window_, SW_SHOW);
        ::UpdateWindow(window_);
        SetForegroundWindow(window_);
        if (cancel_button_) SetFocus(cancel_button_);

        bool repost_quit = false;
        int quit_code = 0;
        MSG message{};
        while (!finished_) {
            const BOOL status = GetMessageW(&message, nullptr, 0, 0);
            if (status <= 0) {
                if (status == 0) {
                    repost_quit = true;
                    quit_code = static_cast<int>(message.wParam);
                }
                finished_ = true;
                break;
            }
            const bool belongs_to_dialog =
                message.hwnd == window_ || (window_ && IsChild(window_, message.hwnd));
            if (belongs_to_dialog && message.message == WM_KEYDOWN) {
                if (message.wParam == VK_ESCAPE) {
                    finish(false);
                    continue;
                }
                if (message.wParam == VK_RETURN) {
                    finish(GetFocus() == confirm_button_);
                    continue;
                }
            }
            if (!window_ || !IsDialogMessageW(window_, &message)) {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        const bool accepted = accepted_ && !repost_quit;
        if (window_ && IsWindow(window_)) DestroyWindow(window_);
        window_ = nullptr;
        confirm_button_ = nullptr;
        cancel_button_ = nullptr;
        if (disabled_owner_ && owner_ && IsWindow(owner_)) {
            EnableWindow(owner_, TRUE);
            SetForegroundWindow(owner_);
        }
        disabled_owner_ = false;
        destroy_theme_resources();
        if (repost_quit) PostQuitMessage(quit_code);
        return accepted;
    }

private:
    inline static constexpr wchar_t window_class_name[] =
        L"ScreenTranslate.Native.InstallConfirmation.v1";
    static constexpr int id_confirm = 8201;

    static LRESULT CALLBACK window_proc(HWND window, UINT message,
                                        WPARAM wparam, LPARAM lparam) {
        auto* self = reinterpret_cast<InstallConfirmationDialog*>(
            GetWindowLongPtrW(window, GWLP_USERDATA));
        if (message == WM_NCCREATE) {
            const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
            self = static_cast<InstallConfirmationDialog*>(create->lpCreateParams);
            self->window_ = window;
            SetWindowLongPtrW(window, GWLP_USERDATA,
                              reinterpret_cast<LONG_PTR>(self));
        }
        try {
            return self ? self->handle_message(message, wparam, lparam)
                        : DefWindowProcW(window, message, wparam, lparam);
        } catch (...) {
            return message == WM_NCCREATE ? FALSE : message == WM_CREATE ? -1 : 0;
        }
    }

    LRESULT handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
        switch (message) {
        case WM_CREATE:
            return create_controls() ? 0 : -1;
        case WM_SIZE:
            layout_controls();
            InvalidateRect(window_, nullptr, FALSE);
            return 0;
        case WM_DPICHANGED: {
            dpi_ = HIWORD(wparam);
            const auto* suggested = reinterpret_cast<const RECT*>(lparam);
            SetWindowPos(window_, nullptr, suggested->left, suggested->top,
                         suggested->right - suggested->left,
                         suggested->bottom - suggested->top,
                         SWP_NOACTIVATE | SWP_NOZORDER);
            create_theme_resources();
            set_font(confirm_button_, font_);
            set_font(cancel_button_, font_);
            layout_controls();
            RedrawWindow(window_, nullptr, nullptr,
                         RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE |
                             RDW_UPDATENOW);
            return 0;
        }
        case WM_COMMAND:
            if (HIWORD(wparam) == BN_CLICKED) {
                if (LOWORD(wparam) == id_confirm) {
                    finish(true);
                    return 0;
                }
                if (LOWORD(wparam) == IDCANCEL) {
                    finish(false);
                    return 0;
                }
            }
            break;
        case WM_DRAWITEM:
            if (const auto* item = reinterpret_cast<const DRAWITEMSTRUCT*>(lparam)) {
                return draw_button(*item);
            }
            return FALSE;
        case WM_PAINT:
            paint();
            return 0;
        case WM_ERASEBKGND:
            return 1;
        case WM_KEYDOWN:
            if (wparam == VK_ESCAPE) {
                finish(false);
                return 0;
            }
            break;
        case WM_CLOSE:
            finish(false);
            return 0;
        case WM_NCDESTROY: {
            const HWND destroyed = window_;
            finished_ = true;
            SetWindowLongPtrW(destroyed, GWLP_USERDATA, 0);
            window_ = nullptr;
            return DefWindowProcW(destroyed, message, wparam, lparam);
        }
        default:
            break;
        }
        return DefWindowProcW(window_, message, wparam, lparam);
    }

    bool create_theme_resources() {
        destroy_theme_resources();
        const auto make_font = [&](int pixels, int weight) {
            return CreateFontW(-px(pixels, dpi_), 0, 0, 0, weight,
                               FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                               OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                               CLEARTYPE_QUALITY, DEFAULT_PITCH,
                               L"Microsoft YaHei UI");
        };
        font_ = make_font(13, FW_NORMAL);
        title_font_ = make_font(17, FW_SEMIBOLD);
        small_font_ = make_font(11, FW_NORMAL);
        background_ = CreateSolidBrush(color_page);
        const bool ready = font_ && title_font_ && small_font_ && background_;
        if (!ready) destroy_theme_resources();
        return ready;
    }

    void destroy_theme_resources() noexcept {
        for (auto** font : {&font_, &title_font_, &small_font_}) {
            if (*font) DeleteObject(*font);
            *font = nullptr;
        }
        if (background_) DeleteObject(background_);
        background_ = nullptr;
    }

    bool create_controls() {
        const auto make_button = [&](const wchar_t* text, int identifier) {
            HWND button = CreateWindowExW(
                0, L"BUTTON", text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
                0, 0, 1, 1, window_,
                reinterpret_cast<HMENU>(static_cast<INT_PTR>(identifier)),
                instance_, nullptr);
            set_font(button, font_);
            return button;
        };
        confirm_button_ = make_button(L"继续安装", id_confirm);
        cancel_button_ = make_button(L"取消", IDCANCEL);
        if (!confirm_button_ || !cancel_button_) return false;
        layout_controls();
        return true;
    }

    void layout_controls() const noexcept {
        if (!window_) return;
        RECT client{};
        GetClientRect(window_, &client);
        const int margin = px(22, dpi_);
        const int bottom = client.bottom - px(16, dpi_);
        const int height = px(36, dpi_);
        const int cancel_width = px(92, dpi_);
        const int confirm_width = px(122, dpi_);
        const int gap = px(10, dpi_);
        const int cancel_left = client.right - margin - cancel_width;
        if (cancel_button_) {
            SetWindowPos(cancel_button_, nullptr, cancel_left, bottom - height,
                         cancel_width, height, SWP_NOACTIVATE | SWP_NOZORDER);
        }
        if (confirm_button_) {
            SetWindowPos(confirm_button_, nullptr,
                         cancel_left - gap - confirm_width, bottom - height,
                         confirm_width, height, SWP_NOACTIVATE | SWP_NOZORDER);
        }
    }

    void paint() const {
        PAINTSTRUCT state{};
        HDC target = BeginPaint(window_, &state);
        RECT client{};
        GetClientRect(window_, &client);
        HDC dc = CreateCompatibleDC(target);
        HBITMAP bitmap = CreateCompatibleBitmap(
            target, std::max(1L, client.right), std::max(1L, client.bottom));
        if (!dc || !bitmap) {
            if (bitmap) DeleteObject(bitmap);
            if (dc) DeleteDC(dc);
            FillRect(target, &client, background_);
            EndPaint(window_, &state);
            return;
        }
        const auto old_bitmap = SelectObject(dc, bitmap);
        FillRect(dc, &client, background_);

        const int margin = px(22, dpi_);
        RECT icon{margin, px(20, dpi_), margin + px(42, dpi_), px(62, dpi_)};
        fill_round_rect(dc, icon, px(21, dpi_), RGB(55, 46, 27), color_warn);
        draw_text(dc, L"!", icon, title_font_, color_warn,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        const int heading_left = icon.right + px(14, dpi_);
        draw_text(dc, L"准备安装更新",
                  RECT{heading_left, px(17, dpi_), client.right - margin,
                       px(43, dpi_)},
                  title_font_, RGB(242, 243, 246),
                  DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        draw_text(dc, L"请确认后再退出当前版本",
                  RECT{heading_left, px(43, dpi_), client.right - margin,
                       px(64, dpi_)},
                  small_font_, color_text_dim,
                  DT_LEFT | DT_VCENTER | DT_SINGLELINE);

        RECT card{margin, px(78, dpi_), client.right - margin, px(254, dpi_)};
        fill_round_rect(dc, card, px(8, dpi_), color_card, color_line);
        draw_text(dc, prompt_,
                  RECT{card.left + px(17, dpi_), card.top + px(14, dpi_),
                       card.right - px(17, dpi_), card.bottom - px(13, dpi_)},
                  font_, color_text,
                  DT_LEFT | DT_TOP | DT_WORDBREAK | DT_EDITCONTROL);

        HPEN separator = CreatePen(PS_SOLID, 1, color_line);
        const auto old_pen = SelectObject(dc, separator);
        MoveToEx(dc, margin, px(270, dpi_), nullptr);
        LineTo(dc, client.right - margin, px(270, dpi_));
        SelectObject(dc, old_pen);
        DeleteObject(separator);

        BitBlt(target, 0, 0, client.right, client.bottom, dc, 0, 0, SRCCOPY);
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        EndPaint(window_, &state);
    }

    LRESULT draw_button(const DRAWITEMSTRUCT& item) const {
        if (item.CtlType != ODT_BUTTON) return FALSE;
        RECT rect = item.rcItem;
        const bool primary = item.hwndItem == confirm_button_;
        const bool disabled = (item.itemState & ODS_DISABLED) != 0;
        const bool pressed = (item.itemState & ODS_SELECTED) != 0;
        const bool hot = (item.itemState & ODS_HOTLIGHT) != 0;
        const bool focused = (item.itemState & ODS_FOCUS) != 0;
        FillRect(item.hDC, &rect, background_);

        COLORREF fill = color_page;
        COLORREF border = color_line_high;
        COLORREF foreground = color_text_dim;
        if (disabled) {
            fill = RGB(32, 35, 41);
            border = RGB(44, 48, 56);
            foreground = color_text_faint;
        } else if (primary) {
        fill = pressed ? accent_ : hot ? adjust_rgb(accent_, 18) : accent_;
            border = fill;
            foreground = RGB(14, 16, 19);
        } else {
            fill = pressed ? RGB(35, 38, 45)
                           : hot ? RGB(31, 34, 41) : color_page;
            foreground = hot || focused ? color_text : color_text_dim;
        }
        fill_round_rect(item.hDC, rect, px(6, dpi_), fill,
                        focused ? accent_ : border);
        const auto label = text_of(item.hwndItem);
        draw_text(item.hDC, label, rect, font_, foreground,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (focused) {
            RECT focus = rect;
            InflateRect(&focus, -px(3, dpi_), -px(3, dpi_));
            DrawFocusRect(item.hDC, &focus);
        }
        return TRUE;
    }

    void finish(bool accepted) noexcept {
        accepted_ = accepted;
        finished_ = true;
        if (window_) ShowWindow(window_, SW_HIDE);
    }

    HINSTANCE instance_{};
    HWND owner_{};
    HWND window_{};
    HWND confirm_button_{};
    HWND cancel_button_{};
    HFONT font_{};
    HFONT title_font_{};
    HFONT small_font_{};
    HBRUSH background_{};
    std::wstring prompt_;
    COLORREF accent_{RGB(40, 199, 111)};
    int dpi_{96};
    bool accepted_{};
    bool finished_{};
    bool disabled_owner_{};
};

}  // namespace

struct UpdateWindow::WorkerDispatch {
    std::mutex mutex;
    HWND target{};
};

UpdateWindow::UpdateWindow(HINSTANCE instance, ConfigStore& config)
    : instance_(instance), config_(config) {}

UpdateWindow::~UpdateWindow() {
    shutdown_worker(2500);
    invalidate_worker_dispatch();
    discard_worker_messages(window_);
    if (window_ && IsWindow(window_)) DestroyWindow(window_);
    for (auto** font : {&font_, &title_font_, &state_font_, &small_font_}) {
        if (*font) DeleteObject(*font);
    }
    if (background_) DeleteObject(background_);
    if (card_background_) DeleteObject(card_background_);
}

bool UpdateWindow::post_worker_message(
    const std::shared_ptr<WorkerDispatch>& dispatch,
    UINT message, LPARAM lparam) noexcept {
    if (!dispatch) return false;
    std::lock_guard lock(dispatch->mutex);
    return dispatch->target &&
           PostMessageW(dispatch->target, message, 0, lparam) != FALSE;
}

void UpdateWindow::invalidate_worker_dispatch() noexcept {
    const auto dispatch = worker_dispatch_;
    if (!dispatch) return;
    std::lock_guard lock(dispatch->mutex);
    dispatch->target = nullptr;
}

bool UpdateWindow::preprocess_message(MSG& message) {
    const HWND dialog = window_;
    if (!dialog || !IsWindow(dialog) || !IsWindowVisible(dialog)) return false;
    return IsDialogMessageW(dialog, &message) != FALSE;
}

void UpdateWindow::show(HWND owner) {
    if (window_) {
        refresh_repository_control();
        ShowWindow(window_, SW_RESTORE);
        SetWindowPos(window_, HWND_TOP, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetForegroundWindow(window_);
        if (!worker_.joinable()) start_check();
        return;
    }
    owner_ = owner;
    available_.reset();
    downloaded_.reset();
    pending_install_prompt_ = false;
    details_visible_ = false;
    progress_visible_ = false;
    progress_received_ = 0;
    progress_total_ = 0;
    check_role_ = ButtonRole::primary;
    release_role_ = ButtonRole::ghost;
    action_role_ = ButtonRole::primary;
    if (config_.string(L"updates.repository_url").empty()) {
        view_state_ = ViewState::unconfigured;
        state_title_ = L"此构建未配置更新";
        state_message_ = L"请使用 GitHub Release 中的正式安装版或便携版。";
    } else {
        const auto channel = config_.string(L"updates.channel", L"stable") == L"stable"
            ? L"稳定版" : L"预览版";
        view_state_ = ViewState::idle;
        state_title_ = L"准备检查更新";
        state_message_ = L"将从 GitHub Release 获取" + std::wstring(channel) + L"发布信息。";
    }
    create_window(owner);
    const HWND window = window_;
    if (!window) throw AppError("update window creation returned no window");
    ShowWindow(window, SW_SHOW);
    SetForegroundWindow(window);
    start_check();
}

void UpdateWindow::refresh_appearance() {
    if (!window_ || !IsWindow(window_)) return;
    refresh_repository_control();
    RedrawWindow(window_, nullptr, nullptr,
                 RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW);
}

void UpdateWindow::self_test(HWND owner) {
    if (window_) throw AppError("update self-test window already exists");
    view_state_ = ViewState::idle;
    state_title_ = L"准备检查更新";
    state_message_ = L"将从 GitHub Release 获取稳定版发布信息。";
    details_visible_ = false;
    progress_visible_ = false;
    create_window(owner);
    if (!window_ || !check_button_ || !release_button_ || !action_button_ ||
        !state_message_control_ || !published_value_control_ ||
        !artifact_value_control_ || !source_control_ || !repository_tooltip_ ||
        !font_ || !title_font_ || !background_) {
        throw AppError("update self-test did not create required controls");
    }
    for (const HWND control : {state_message_control_, published_value_control_,
                               artifact_value_control_, source_control_}) {
        if ((GetWindowLongPtrW(control, GWL_STYLE) & WS_TABSTOP) != 0) {
            throw AppError("selectable update text entered the keyboard tab order");
        }
    }
    SendMessageW(state_message_control_, EM_SETSEL, 0, 2);
    DWORD selection_start = 0;
    DWORD selection_end = 0;
    SendMessageW(state_message_control_, EM_GETSEL,
                 reinterpret_cast<WPARAM>(&selection_start),
                 reinterpret_cast<LPARAM>(&selection_end));
    if (selection_start != 0 || selection_end != 2 ||
        SendMessageW(repository_tooltip_, TTM_GETTOOLCOUNT, 0, 0) != 1) {
        throw AppError("update selectable text or repository tooltip self-test failed");
    }
    DestroyWindow(window_);
    window_ = nullptr;
}

void UpdateWindow::create_window(HWND owner) {
    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.lpfnWndProc = &UpdateWindow::window_proc;
    description.style = CS_HREDRAW | CS_VREDRAW;
    description.hInstance = instance_;
    description.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    description.hIcon = LoadIconW(instance_, MAKEINTRESOURCEW(IDI_APP_ICON));
    description.hIconSm = description.hIcon;
    description.lpszClassName = class_name;
    if (!RegisterClassExW(&description) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register update window");
    }

    RECT owner_rect{};
    const bool usable_owner = owner && IsWindowVisible(owner) && GetWindowRect(owner, &owner_rect) &&
        owner_rect.right - owner_rect.left >= 100 && owner_rect.bottom - owner_rect.top >= 100;
    HMONITOR monitor{};
    if (usable_owner) {
        monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
    } else {
        POINT cursor{};
        GetCursorPos(&cursor);
        monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO info{sizeof(info)};
        if (GetMonitorInfoW(monitor, &info)) {
            owner_rect = info.rcWork;
        } else {
            owner_rect = {0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)};
        }
    }
    UINT monitor_dpi_x = 96;
    UINT monitor_dpi_y = 96;
    if (!monitor || FAILED(GetDpiForMonitor(
            monitor, MDT_EFFECTIVE_DPI, &monitor_dpi_x, &monitor_dpi_y))) {
        monitor_dpi_x = GetDpiForSystem();
    }
    dpi_ = usable_owner ? static_cast<int>(GetDpiForWindow(owner))
                        : static_cast<int>(monitor_dpi_x);

    const DWORD style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU |
                        WS_THICKFRAME | WS_CLIPCHILDREN;
    constexpr DWORD ex_style = WS_EX_DLGMODALFRAME;
    RECT desired{0, 0, px(580, dpi_), px(260, dpi_)};
    AdjustWindowRectExForDpi(&desired, style, FALSE, ex_style, static_cast<UINT>(dpi_));
    const int width = desired.right - desired.left;
    const int height = desired.bottom - desired.top;
    const int x = owner_rect.left + std::max(0L, (owner_rect.right - owner_rect.left - width) / 2);
    const int y = owner_rect.top + std::max(0L, (owner_rect.bottom - owner_rect.top - height) / 2);
    window_error_.clear();
    auto dispatch = std::make_shared<WorkerDispatch>();
    window_ = CreateWindowExW(
        ex_style, class_name, L"ScreenTranslate · 检查更新", style,
        x, y, width, height, owner, nullptr, instance_, this);
    if (!window_) {
        if (!window_error_.empty()) throw AppError(window_error_);
        throw_last_error("create update window");
    }
    dispatch->target = window_;
    worker_dispatch_ = std::move(dispatch);
    enable_dark_frame(window_);
}

void UpdateWindow::create_theme_resources() {
    for (auto** font : {&font_, &title_font_, &state_font_, &small_font_}) {
        if (*font) DeleteObject(*font);
        *font = nullptr;
    }
    if (background_) DeleteObject(background_);
    if (card_background_) DeleteObject(card_background_);
    background_ = nullptr;
    card_background_ = nullptr;

    const auto make_font = [&](int pixels, int weight) {
        return CreateFontW(-px(pixels, dpi_), 0, 0, 0, weight, FALSE, FALSE, FALSE,
                           DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                           CLEARTYPE_QUALITY, DEFAULT_PITCH, L"Microsoft YaHei UI");
    };
    font_ = make_font(13, FW_NORMAL);
    title_font_ = make_font(18, FW_SEMIBOLD);
    state_font_ = make_font(14, FW_SEMIBOLD);
    small_font_ = make_font(11, FW_NORMAL);
    background_ = CreateSolidBrush(color_page);
    card_background_ = CreateSolidBrush(color_card);
    if (!font_ || !title_font_ || !state_font_ || !small_font_ ||
        !background_ || !card_background_) {
        throw AppError("cannot create update window theme resources");
    }
}

void UpdateWindow::create_controls() {
    INITCOMMONCONTROLSEX common_controls{};
    common_controls.dwSize = sizeof(common_controls);
    common_controls.dwICC = ICC_WIN95_CLASSES;
    if (!InitCommonControlsEx(&common_controls)) {
        throw_last_error("initialize update common controls");
    }
    create_theme_resources();
    auto make_button = [&](const wchar_t* text, int identifier) {
        HWND button = CreateWindowExW(
            0, L"BUTTON", text,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
            0, 0, 1, 1, window_,
            reinterpret_cast<HMENU>(static_cast<INT_PTR>(identifier)), instance_, nullptr);
        if (!button) throw_last_error("create update button");
        set_font(button, font_);
        return button;
    };
    const auto make_selectable_text = [&](DWORD style, HFONT font) {
        HWND control = CreateWindowExW(
            0, L"EDIT", L"",
            WS_CHILD | WS_VISIBLE | ES_LEFT | ES_READONLY | style,
            0, 0, 1, 1, window_, nullptr, instance_, nullptr);
        if (!control) throw_last_error("create selectable update text");
        set_font(control, font);
        SendMessageW(control, EM_SETMARGINS,
                     EC_LEFTMARGIN | EC_RIGHTMARGIN, MAKELPARAM(0, 0));
        SetWindowTheme(control, L"DarkMode_Explorer", nullptr);
        return control;
    };
    check_button_ = make_button(L"检查更新", id_check);
    release_button_ = make_button(L"发布说明", id_release);
    action_button_ = make_button(L"下载并安装", id_action);
    state_message_control_ = make_selectable_text(ES_MULTILINE, small_font_);
    published_value_control_ = make_selectable_text(0, font_);
    artifact_value_control_ = make_selectable_text(ES_MULTILINE, font_);
    source_control_ = make_selectable_text(0, small_font_);
    ShowWindow(release_button_, SW_HIDE);
    ShowWindow(action_button_, SW_HIDE);

    repository_tooltip_ = CreateWindowExW(
        WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr,
        WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
        CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
        window_, nullptr, instance_, nullptr);
    if (!repository_tooltip_) throw_last_error("create repository tooltip");
    SetWindowTheme(repository_tooltip_, L"DarkMode_Explorer", nullptr);
    SendMessageW(repository_tooltip_, TTM_SETTIPBKCOLOR, color_input, 0);
    SendMessageW(repository_tooltip_, TTM_SETTIPTEXTCOLOR, color_text, 0);
    SendMessageW(repository_tooltip_, TTM_SETMAXTIPWIDTH, 0, px(640, dpi_));
    SetWindowPos(repository_tooltip_, HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);

    repository_tooltip_text_ = config_.string(L"updates.repository_url");
    TOOLINFOW tool{};
    tool.cbSize = TTTOOLINFOW_V2_SIZE;
    tool.uFlags = TTF_SUBCLASS | TTF_TRANSPARENT;
    tool.hwnd = source_control_;
    tool.uId = 1;
    GetClientRect(source_control_, &tool.rect);
    tool.lpszText = repository_tooltip_text_.data();
    if (!SendMessageW(repository_tooltip_, TTM_ADDTOOLW, 0,
                      reinterpret_cast<LPARAM>(&tool))) {
        throw AppError("cannot attach repository tooltip");
    }
    refresh_repository_control();
    layout_controls();
}

void UpdateWindow::sync_text_controls() {
    const auto set_text = [](HWND control, const std::wstring& value) {
        if (control && text_of(control) != value) {
            SetWindowTextW(control, value.c_str());
        }
    };
    set_text(state_message_control_, state_message_);
    set_text(published_value_control_, published_value_);
    set_text(artifact_value_control_, artifact_value_);
}

void UpdateWindow::refresh_repository_control() {
    if (!source_control_) return;
    const auto repository = config_.string(L"updates.repository_url");
    const auto label = source_label(repository);
    if (text_of(source_control_) != label) {
        SetWindowTextW(source_control_, label.c_str());
    }
    repository_tooltip_text_ = repository;
    if (!repository_tooltip_) return;
    TOOLINFOW tool{};
    tool.cbSize = TTTOOLINFOW_V2_SIZE;
    tool.uFlags = TTF_SUBCLASS | TTF_TRANSPARENT;
    tool.hwnd = source_control_;
    tool.uId = 1;
    GetClientRect(source_control_, &tool.rect);
    tool.lpszText = repository_tooltip_text_.data();
    SendMessageW(repository_tooltip_, TTM_UPDATETIPTEXTW, 0,
                 reinterpret_cast<LPARAM>(&tool));
}

void UpdateWindow::layout_controls() {
    if (!window_) return;
    sync_text_controls();
    RECT client{};
    GetClientRect(window_, &client);
    const int margin = px(24, dpi_);
    const int button_height = px(36, dpi_);
    const int button_y = client.bottom - px(20, dpi_) - button_height;
    int x = client.right - margin;
    auto place = [&](HWND button, int width) {
        if (!button || !IsWindowVisible(button)) return;
        x -= px(width, dpi_);
        MoveWindow(button, x, button_y, px(width, dpi_), button_height, TRUE);
        x -= px(9, dpi_);
    };
    place(action_button_, 132);
    place(release_button_, 104);
    place(check_button_, 112);

    const int state_top = px(76, dpi_);
    const int state_height = px(progress_visible_ ? 100 : 76, dpi_);
    const int state_text_left = margin + px(63, dpi_);
    const int state_right = client.right - margin - px(17, dpi_);
    const int state_message_top = state_top + px(34, dpi_);
    const int state_message_bottom = state_top + state_height -
                                     px(progress_visible_ ? 29 : 9, dpi_);
    MoveWindow(state_message_control_, state_text_left, state_message_top,
               std::max(1, state_right - state_text_left),
               std::max(1, state_message_bottom - state_message_top), TRUE);

    if (details_visible_) {
        const int details_top = state_top + state_height + px(16, dpi_);
        const int value_left = margin + px(96, dpi_);
        const int value_right = client.right - margin - px(17, dpi_);
        int row = details_top + px(12, dpi_);
        if (!published_value_.empty()) {
            MoveWindow(published_value_control_, value_left, row,
                       std::max(1, value_right - value_left), px(18, dpi_), TRUE);
            ShowWindow(published_value_control_, SW_SHOW);
            row += px(25, dpi_);
        } else {
            ShowWindow(published_value_control_, SW_HIDE);
        }
        MoveWindow(artifact_value_control_, value_left, row,
                   std::max(1, value_right - value_left), px(28, dpi_), TRUE);
        ShowWindow(artifact_value_control_, SW_SHOW);
    } else {
        ShowWindow(published_value_control_, SW_HIDE);
        ShowWindow(artifact_value_control_, SW_HIDE);
    }

    const int actions_top = client.bottom - px(56, dpi_);
    MoveWindow(source_control_, margin, actions_top,
               std::max(1, x - margin), px(36, dpi_), TRUE);
    if (repository_tooltip_) {
        TOOLINFOW tool{};
        tool.cbSize = TTTOOLINFOW_V2_SIZE;
        tool.uFlags = TTF_SUBCLASS | TTF_TRANSPARENT;
        tool.hwnd = source_control_;
        tool.uId = 1;
        GetClientRect(source_control_, &tool.rect);
        SendMessageW(repository_tooltip_, TTM_NEWTOOLRECTW, 0,
                     reinterpret_cast<LPARAM>(&tool));
    }
}

void UpdateWindow::resize_for_content() {
    if (!window_) return;
    const int client_height = details_visible_
        ? (progress_visible_ ? 382 : 360)
        : (progress_visible_ ? 282 : 260);
    RECT current_client{};
    RECT window_rect{};
    GetClientRect(window_, &current_client);
    GetWindowRect(window_, &window_rect);
    RECT desired{0, 0, current_client.right, px(client_height, dpi_)};
    AdjustWindowRectExForDpi(
        &desired,
        static_cast<DWORD>(GetWindowLongPtrW(window_, GWL_STYLE)), FALSE,
        static_cast<DWORD>(GetWindowLongPtrW(window_, GWL_EXSTYLE)),
        static_cast<UINT>(dpi_));
    SetWindowPos(window_, nullptr, window_rect.left, window_rect.top,
                 window_rect.right - window_rect.left, desired.bottom - desired.top,
                 SWP_NOACTIVATE | SWP_NOZORDER);
}

void UpdateWindow::paint() {
    PAINTSTRUCT state{};
    HDC target = BeginPaint(window_, &state);
    if (!target) {
        EndPaint(window_, &state);
        return;
    }
    RECT client{};
    GetClientRect(window_, &client);
    if (client.right <= client.left || client.bottom <= client.top) {
        EndPaint(window_, &state);
        return;
    }
    HDC dc = CreateCompatibleDC(target);
    HBITMAP bitmap = dc ? CreateCompatibleBitmap(target, client.right, client.bottom) : nullptr;
    if (!dc || !bitmap) {
        FillRect(target, &client, background_);
        if (bitmap) DeleteObject(bitmap);
        if (dc) DeleteDC(dc);
        EndPaint(window_, &state);
        return;
    }
    const auto old_bitmap = SelectObject(dc, bitmap);
    FillRect(dc, &client, background_);

    const COLORREF accent = parse_rgb_color(
        config_.string(L"appearance.accent", L"#28C76F"));
    const int margin = px(24, dpi_);

    RECT mark{margin, px(20, dpi_), margin + px(38, dpi_), px(58, dpi_)};
    RECT bubble{mark.left, mark.top, mark.right, mark.bottom - px(5, dpi_)};
    fill_round_rect(dc, bubble, px(9, dpi_), accent, accent);
    POINT tail[3]{
        {mark.left + px(5, dpi_), mark.bottom - px(10, dpi_)},
        {mark.left + px(5, dpi_), mark.bottom},
        {mark.left + px(14, dpi_), mark.bottom - px(6, dpi_)},
    };
    HBRUSH mark_brush = CreateSolidBrush(accent);
    HPEN mark_pen = CreatePen(PS_SOLID, 1, accent);
    const auto old_brush = SelectObject(dc, mark_brush);
    const auto old_pen = SelectObject(dc, mark_pen);
    Polygon(dc, tail, static_cast<int>(std::size(tail)));
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(mark_pen);
    DeleteObject(mark_brush);
    draw_text(dc, L"译", bubble, state_font_, RGB(14, 16, 19),
              DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    const int heading_left = mark.right + px(12, dpi_);
    draw_text(dc, L"ScreenTranslate",
              RECT{heading_left, px(17, dpi_), client.right - margin, px(42, dpi_)},
              title_font_, RGB(242, 243, 246),
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    const auto channel = config_.string(L"updates.channel", L"stable") == L"stable"
        ? L"稳定版" : L"预览版";
    const auto version = L"当前版本 " + std::wstring(native_version) + L"  ·  " + channel;
    draw_text(dc, version,
              RECT{heading_left, px(42, dpi_), client.right - margin, px(62, dpi_)},
              small_font_, color_text_dim,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    COLORREF state_color = color_text_dim;
    std::wstring_view state_glyph = L"i";
    switch (view_state_) {
    case ViewState::checking: state_color = accent; state_glyph = L"↻"; break;
    case ViewState::current: state_color = color_ok; state_glyph = L"✓"; break;
    case ViewState::available: state_color = accent; state_glyph = L"i"; break;
    case ViewState::downloading: state_color = accent; state_glyph = L"↓"; break;
    case ViewState::ready: state_color = color_ok; state_glyph = L"✓"; break;
    case ViewState::installing: state_color = color_warn; state_glyph = L"i"; break;
    case ViewState::error: state_color = color_bad; state_glyph = L"!"; break;
    case ViewState::unconfigured: state_color = color_warn; state_glyph = L"!"; break;
    case ViewState::idle: break;
    }

    const int state_top = px(76, dpi_);
    const int state_height = px(progress_visible_ ? 100 : 76, dpi_);
    RECT state_card{margin, state_top, client.right - margin, state_top + state_height};
    fill_round_rect(dc, state_card, px(10, dpi_), color_card, color_line);
    RECT icon_rect{state_card.left + px(17, dpi_), state_card.top + px(15, dpi_),
                   state_card.left + px(49, dpi_), state_card.top + px(47, dpi_)};
    draw_text(dc, state_glyph, icon_rect, title_font_, state_color,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    const int state_text_left = state_card.left + px(63, dpi_);
    draw_text(dc, state_title_,
              RECT{state_text_left, state_card.top + px(11, dpi_),
                   state_card.right - px(17, dpi_), state_card.top + px(35, dpi_)},
              state_font_, state_color, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

    if (progress_visible_) {
        RECT progress{state_text_left, state_card.bottom - px(23, dpi_),
                      state_card.right - px(17, dpi_), state_card.bottom - px(7, dpi_)};
        fill_round_rect(dc, progress, px(4, dpi_), color_input, color_line_high);
        if (progress_total_ > 0 && progress_received_ > 0) {
            const auto bounded = std::min(progress_received_, progress_total_);
            const int width = progress.right - progress.left;
            const int completed = static_cast<int>(
                std::min<std::uint64_t>(static_cast<std::uint64_t>(width),
                    bounded * static_cast<std::uint64_t>(width) / progress_total_));
            if (completed > 0) {
                RECT chunk{progress.left + 1, progress.top + 1,
                           std::min(progress.right - 1, progress.left + completed),
                           progress.bottom - 1};
                fill_round_rect(dc, chunk, px(3, dpi_), accent, accent);
            }
        }
    }

    if (details_visible_) {
        const int details_top = state_card.bottom + px(16, dpi_);
        RECT details{margin, details_top, client.right - margin, details_top + px(112, dpi_)};
        fill_round_rect(dc, details, px(10, dpi_), color_card, color_line);
        const int label_left = details.left + px(17, dpi_);
        const int value_left = details.left + px(96, dpi_);
        const int value_right = details.right - px(17, dpi_);
        int row = details.top + px(12, dpi_);
        const auto detail_key = [&](std::wstring_view key, int height) {
            draw_text(dc, key,
                      RECT{label_left, row, value_left - px(12, dpi_), row + px(height, dpi_)},
                      font_, color_text_dim, DT_LEFT | DT_TOP | DT_SINGLELINE);
            row += px(height + 7, dpi_);
        };
        if (!published_value_.empty()) detail_key(L"发布时间", 18);
        detail_key(L"安装包", 28);
        draw_text(dc, L"完整性",
                  RECT{label_left, row, value_left - px(12, dpi_), row + px(18, dpi_)},
                  font_, color_text_dim, DT_LEFT | DT_TOP | DT_SINGLELINE);
        draw_text(dc, verification_value_,
                  RECT{value_left, row, value_right, row + px(18, dpi_)},
                  font_, color_text_dim, DT_LEFT | DT_TOP | DT_SINGLELINE);
    }

    BitBlt(target, 0, 0, client.right, client.bottom, dc, 0, 0, SRCCOPY);
    SelectObject(dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(dc);
    EndPaint(window_, &state);
}

LRESULT UpdateWindow::draw_item(const DRAWITEMSTRUCT& item) {
    if (item.CtlType != ODT_BUTTON) return FALSE;
    HDC dc = item.hDC;
    RECT rect = item.rcItem;
    const bool disabled = (item.itemState & ODS_DISABLED) != 0;
    const bool pressed = (item.itemState & ODS_SELECTED) != 0;
    const bool hot = (item.itemState & ODS_HOTLIGHT) != 0;
    const bool focused = (item.itemState & ODS_FOCUS) != 0;
    const COLORREF accent = parse_rgb_color(
        config_.string(L"appearance.accent", L"#28C76F"));
    ButtonRole role = ButtonRole::ghost;
    if (item.hwndItem == check_button_) role = check_role_;
    else if (item.hwndItem == release_button_) role = release_role_;
    else if (item.hwndItem == action_button_) role = action_role_;

    FillRect(dc, &rect, background_);
    COLORREF fill = color_page;
    COLORREF border = color_line_high;
    COLORREF foreground = color_text_dim;
    if (disabled) {
        fill = RGB(32, 35, 41);
        border = RGB(44, 48, 56);
        foreground = color_text_faint;
    } else if (role == ButtonRole::primary) {
        fill = pressed ? accent : hot ? adjust_rgb(accent, 18) : accent;
        border = fill;
        foreground = RGB(14, 16, 19);
    } else {
        fill = pressed ? RGB(35, 38, 45) : hot ? RGB(31, 34, 41) : color_page;
        foreground = hot ? color_text : color_text_dim;
    }
    fill_round_rect(dc, rect, px(6, dpi_), fill, focused ? accent : border);

    std::wstring glyph = L"↻";
    if (item.hwndItem == release_button_) {
        glyph = L"◎";
    } else if (item.hwndItem == action_button_) {
        const auto label = text_of(item.hwndItem);
        if (label.find(L"取消") != std::wstring::npos) glyph = L"×";
        else if (label.find(L"安装") != std::wstring::npos ||
                 label.find(L"启动") != std::wstring::npos) glyph = L"✓";
        else glyph = L"↓";
    }
    const auto label = text_of(item.hwndItem);
    SIZE label_size{};
    const auto old_font = SelectObject(dc, font_);
    GetTextExtentPoint32W(dc, label.c_str(), static_cast<int>(label.size()), &label_size);
    SelectObject(dc, old_font);
    const int icon_width = px(16, dpi_);
    const int gap = px(6, dpi_);
    const int total = icon_width + gap + label_size.cx;
    const int button_width = static_cast<int>(rect.right - rect.left);
    const int left = static_cast<int>(rect.left) + std::max(0, (button_width - total) / 2);
    draw_text(dc, glyph,
              RECT{left, rect.top, left + icon_width, rect.bottom},
              font_, foreground, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    draw_text(dc, label,
              RECT{left + icon_width + gap, rect.top, rect.right - px(7, dpi_), rect.bottom},
              font_, foreground, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    if (focused) {
        RECT focus = rect;
        InflateRect(&focus, -px(3, dpi_), -px(3, dpi_));
        DrawFocusRect(dc, &focus);
    }
    return TRUE;
}

void UpdateWindow::set_state(ViewState state, std::wstring title, std::wstring message) {
    view_state_ = state;
    state_title_ = std::move(title);
    state_message_ = std::move(message);
    sync_text_controls();
    if (window_) InvalidateRect(window_, nullptr, FALSE);
}

void UpdateWindow::set_state_message(std::wstring message) {
    state_message_ = std::move(message);
    sync_text_controls();
    if (window_) InvalidateRect(window_, nullptr, FALSE);
}

void UpdateWindow::set_details_visible(bool visible) {
    if (details_visible_ == visible) return;
    details_visible_ = visible;
    resize_for_content();
    layout_controls();
    if (window_) InvalidateRect(window_, nullptr, FALSE);
}

void UpdateWindow::finish_worker() {
    if (worker_.joinable()) worker_.join();
    worker_finished_.reset();
    worker_mode_ = WorkerMode::none;
    EnableWindow(check_button_, TRUE);
}

void UpdateWindow::shutdown_worker(DWORD timeout_ms) noexcept {
    if (!worker_.joinable()) {
        worker_finished_.reset();
        worker_mode_ = WorkerMode::none;
        return;
    }
    worker_.request_stop();
    const ULONGLONG deadline = GetTickCount64() + timeout_ms;
    while (worker_finished_ &&
           !worker_finished_->load(std::memory_order_acquire) &&
           GetTickCount64() < deadline) {
        Sleep(10);
    }
    if (!worker_finished_ ||
        worker_finished_->load(std::memory_order_acquire)) {
        try { worker_.join(); } catch (...) {}
    } else {
        try { worker_.detach(); } catch (...) {}
    }
    worker_finished_.reset();
    worker_mode_ = WorkerMode::none;
}

void UpdateWindow::discard_worker_messages(HWND target) noexcept {
    if (!target) target = window_;
    if (!target) return;
    MSG message{};
    while (PeekMessageW(&message, target, check_completed_message,
                        check_completed_message, PM_REMOVE)) {
        delete reinterpret_cast<CheckResult*>(message.lParam);
    }
    while (PeekMessageW(&message, target, download_completed_message,
                        download_completed_message, PM_REMOVE)) {
        delete reinterpret_cast<DownloadResult*>(message.lParam);
    }
    while (PeekMessageW(&message, target, progress_message,
                        progress_message, PM_REMOVE)) {
        delete reinterpret_cast<std::pair<std::uint64_t, std::uint64_t>*>(message.lParam);
    }
    while (PeekMessageW(&message, target, install_prompt_message,
                        install_prompt_message, PM_REMOVE)) {
    }
}

void UpdateWindow::start_check() {
    if (worker_.joinable()) return;
    const auto manifest = config_.string(L"updates.manifest_url");
    const auto repository = config_.string(L"updates.repository_url");
    const auto channel = config_.string(L"updates.channel", L"stable");
    available_.reset();
    downloaded_.reset();
    pending_install_prompt_ = false;
    progress_visible_ = false;
    progress_received_ = 0;
    progress_total_ = 0;
    published_value_.clear();
    artifact_value_.clear();
    verification_value_.clear();
    ShowWindow(release_button_, SW_HIDE);
    ShowWindow(action_button_, SW_HIDE);
    check_role_ = ButtonRole::primary;
    SetWindowTextW(check_button_, L"检查更新");
    set_details_visible(false);
    if (manifest.empty() || repository.empty()) {
        set_state(ViewState::unconfigured, L"此构建未配置更新",
                  L"请使用 GitHub Release 中的正式安装版或便携版。");
        layout_controls();
        return;
    }
    const auto channel_name = channel == L"stable" ? L"稳定版" : L"预览版";
    set_state(ViewState::checking, L"正在检查更新",
              L"正在读取 GitHub 上的" + std::wstring(channel_name) + L"发布信息，请稍候。");
    SetWindowTextW(check_button_, L"检查中…");
    EnableWindow(check_button_, FALSE);
    worker_mode_ = WorkerMode::check;
    const auto dispatch = worker_dispatch_;
    worker_finished_ = std::make_shared<std::atomic_bool>(false);
    const auto worker_finished = worker_finished_;
    worker_ = std::jthread([dispatch, manifest, repository, channel,
                            worker_finished](std::stop_token stop) noexcept {
        try {
            auto result = std::make_unique<CheckResult>();
            try {
                WinrtApartment apartment(winrt::apartment_type::multi_threaded);
                result->update = check_for_update(manifest, repository, channel, stop);
            } catch (const std::exception& error) {
                result->cancelled = stop.stop_requested();
                if (!result->cancelled) result->error = utf8_to_wide(error.what());
            } catch (...) {
                result->cancelled = stop.stop_requested();
                if (!result->cancelled) result->error = L"检查更新时遇到未知错误";
            }
            auto* raw = result.release();
            if (!UpdateWindow::post_worker_message(
                    dispatch, check_completed_message,
                    reinterpret_cast<LPARAM>(raw))) {
                delete raw;
            }
        } catch (...) {
        }
        worker_finished->store(true, std::memory_order_release);
    });
    layout_controls();
    InvalidateRect(window_, nullptr, FALSE);
}

void UpdateWindow::start_download() {
    if (!available_ || worker_.joinable()) return;
    const auto repository = config_.string(L"updates.repository_url");
    const auto info = *available_;
    pending_install_prompt_ = false;
    progress_visible_ = true;
    progress_received_ = 0;
    progress_total_ = info.artifact.size;
    set_state(ViewState::downloading, L"正在下载 " + info.version,
              L"安装包下载完成后会自动核对大小和 SHA-256。");
    SetWindowTextW(action_button_, L"取消下载");
    action_role_ = ButtonRole::ghost;
    EnableWindow(check_button_, FALSE);
    EnableWindow(release_button_, FALSE);
    EnableWindow(action_button_, TRUE);
    resize_for_content();
    layout_controls();
    worker_mode_ = WorkerMode::download;
    const auto dispatch = worker_dispatch_;
    worker_finished_ = std::make_shared<std::atomic_bool>(false);
    const auto worker_finished = worker_finished_;
    worker_ = std::jthread([dispatch, repository, info,
                            worker_finished](std::stop_token stop) noexcept {
        try {
            auto result = std::make_unique<DownloadResult>();
            try {
                WinrtApartment apartment(winrt::apartment_type::multi_threaded);
                result->path = download_update(info, repository, stop,
                    [dispatch](std::uint64_t received, std::uint64_t total) {
                        auto* progress = new std::pair<std::uint64_t, std::uint64_t>{received, total};
                        if (!UpdateWindow::post_worker_message(
                                dispatch, progress_message,
                                reinterpret_cast<LPARAM>(progress))) {
                            delete progress;
                        }
                    });
            } catch (const std::exception& error) {
                result->cancelled = stop.stop_requested();
                if (!result->cancelled) result->error = utf8_to_wide(error.what());
            } catch (...) {
                result->cancelled = stop.stop_requested();
                if (!result->cancelled) result->error = L"下载更新时遇到未知错误";
            }
            auto* raw = result.release();
            if (!UpdateWindow::post_worker_message(
                    dispatch, download_completed_message,
                    reinterpret_cast<LPARAM>(raw))) {
                delete raw;
            }
        } catch (...) {
        }
        worker_finished->store(true, std::memory_order_release);
    });
}

void UpdateWindow::install() {
    if (!available_ || !downloaded_) return;
    InstallConfirmationDialog confirmation;
    if (!confirmation.show(
            instance_, window_, available_->version,
            parse_rgb_color(config_.string(L"appearance.accent", L"#28C76F")))) {
        return;
    }

    pending_install_prompt_ = false;
    set_state(ViewState::installing, L"正在启动安装程序",
              L"当前版本即将退出，请在安装向导中完成更新。");
    EnableWindow(check_button_, FALSE);
    EnableWindow(release_button_, FALSE);
    EnableWindow(action_button_, FALSE);
    SetWindowTextW(action_button_, L"正在启动…");
    RedrawWindow(window_, nullptr, nullptr,
                 RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW);
    try {
        launch_update_helper(*downloaded_, *available_,
                             config_.string(L"updates.repository_url"), GetCurrentProcessId());
        ShowWindow(window_, SW_HIDE);
        if (quit_callback_) quit_callback_();
    } catch (const std::exception& error) {
        set_state(ViewState::error, L"无法启动安装程序", utf8_to_wide(error.what()));
        EnableWindow(check_button_, TRUE);
        EnableWindow(release_button_, TRUE);
        EnableWindow(action_button_, TRUE);
        action_role_ = ButtonRole::primary;
        SetWindowTextW(action_button_, L"重试安装");
        InvalidateRect(window_, nullptr, FALSE);
    }
}

void UpdateWindow::open_release() {
    if (!available_) return;
    const auto result = reinterpret_cast<INT_PTR>(
        ShellExecuteW(window_, L"open", available_->release_url.c_str(), nullptr, nullptr, SW_SHOWNORMAL));
    if (result <= 32) {
        set_state_message(L"系统浏览器未接受 GitHub 发布页面地址。");
    } else {
        set_state_message(L"已交给系统浏览器打开 GitHub 发布页面。");
    }
}

void UpdateWindow::close() {
    if (worker_.joinable()) worker_.request_stop();
    pending_install_prompt_ = false;
    ShowWindow(window_, SW_HIDE);
}

LRESULT CALLBACK UpdateWindow::window_proc(HWND window, UINT message,
                                            WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<UpdateWindow*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
        self = static_cast<UpdateWindow*>(create->lpCreateParams);
        self->window_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    try {
        return self ? self->handle_message(message, wparam, lparam)
                    : DefWindowProcW(window, message, wparam, lparam);
    } catch (const std::exception& error) {
        if (self) {
            try { self->window_error_ = error.what(); } catch (...) {}
        }
    } catch (...) {
        if (self) {
            try { self->window_error_ = "unknown update window error"; } catch (...) {}
        }
    }
    return message == WM_NCCREATE ? FALSE : message == WM_CREATE ? -1 : 0;
}

LRESULT UpdateWindow::handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_CREATE:
        create_controls();
        return 0;
    case WM_SIZE:
        layout_controls();
        InvalidateRect(window_, nullptr, FALSE);
        return 0;
    case WM_GETMINMAXINFO: {
        const int minimum_height = details_visible_
            ? (progress_visible_ ? 372 : 350)
            : (progress_visible_ ? 272 : 250);
        RECT minimum{0, 0, px(560, dpi_), px(minimum_height, dpi_)};
        AdjustWindowRectExForDpi(
            &minimum,
            static_cast<DWORD>(GetWindowLongPtrW(window_, GWL_STYLE)), FALSE,
            static_cast<DWORD>(GetWindowLongPtrW(window_, GWL_EXSTYLE)),
            static_cast<UINT>(dpi_));
        auto* limits = reinterpret_cast<MINMAXINFO*>(lparam);
        limits->ptMinTrackSize.x = minimum.right - minimum.left;
        limits->ptMinTrackSize.y = minimum.bottom - minimum.top;
        return 0;
    }
    case WM_DPICHANGED: {
        dpi_ = HIWORD(wparam);
        const auto* suggested = reinterpret_cast<RECT*>(lparam);
        SetWindowPos(window_, nullptr, suggested->left, suggested->top,
                     suggested->right - suggested->left, suggested->bottom - suggested->top,
                     SWP_NOACTIVATE | SWP_NOZORDER);
        create_theme_resources();
        set_font(check_button_, font_);
        set_font(release_button_, font_);
        set_font(action_button_, font_);
        set_font(state_message_control_, small_font_);
        set_font(published_value_control_, font_);
        set_font(artifact_value_control_, font_);
        set_font(source_control_, small_font_);
        if (repository_tooltip_) {
            SendMessageW(repository_tooltip_, TTM_SETMAXTIPWIDTH, 0, px(640, dpi_));
        }
        layout_controls();
        RedrawWindow(window_, nullptr, nullptr,
                     RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE | RDW_UPDATENOW);
        return 0;
    }
    case WM_COMMAND: {
        const int identifier = LOWORD(wparam);
        const int notification = HIWORD(wparam);
        if (identifier == IDCANCEL) {
            close();
            return 0;
        }
        if (notification != BN_CLICKED) break;
        if (identifier == id_check) {
            start_check();
            return 0;
        }
        if (identifier == id_release) {
            open_release();
            return 0;
        }
        if (identifier == id_action) {
            if (worker_mode_ == WorkerMode::download && worker_.joinable()) {
                worker_.request_stop();
                set_state(ViewState::downloading, L"正在取消下载",
                          L"正在停止网络请求并清理未完成的安装包。");
                SetWindowTextW(action_button_, L"正在取消…");
                EnableWindow(action_button_, FALSE);
            } else if (downloaded_) {
                install();
            } else {
                start_download();
            }
            return 0;
        }
        break;
    }
    case check_completed_message: {
        std::unique_ptr<CheckResult> result(reinterpret_cast<CheckResult*>(lparam));
        finish_worker();
        if (!result || result->cancelled) return 0;
        if (!result->error.empty()) {
            check_role_ = ButtonRole::primary;
            SetWindowTextW(check_button_, L"重试");
            set_state(ViewState::error, L"检查失败", result->error);
        } else if (!result->update) {
            const auto channel = config_.string(L"updates.channel", L"stable") == L"stable"
                ? L"稳定版" : L"预览版";
            check_role_ = ButtonRole::primary;
            SetWindowTextW(check_button_, L"重新检查");
            set_state(ViewState::current, L"已经是最新版",
                      L"当前安装的 " + std::wstring(native_version) +
                      L" 已是" + std::wstring(channel) + L"的最新版本。");
        } else {
            available_ = std::move(result->update);
            const auto channel = available_->channel == L"stable" ? L"稳定版" : L"预览版";
            set_state(ViewState::available, L"发现新版本 " + available_->version,
                      L"这是" + std::wstring(channel) +
                      L"。可直接下载，程序会自动校验安装包完整性。");
            published_value_ = format_published_at(available_->published_at);
            artifact_value_ = available_->artifact.name + L"（" +
                              format_size(available_->artifact.size) + L"）";
            verification_value_ = L"下载完成后自动校验 SHA-256";
            check_role_ = ButtonRole::ghost;
            action_role_ = ButtonRole::primary;
            SetWindowTextW(check_button_, L"重新检查");
            SetWindowTextW(action_button_, L"下载并安装");
            ShowWindow(release_button_, SW_SHOW);
            ShowWindow(action_button_, SW_SHOW);
            set_details_visible(true);
            if (update_available_callback_) update_available_callback_(*available_);
        }
        layout_controls();
        RedrawWindow(window_, nullptr, nullptr,
                     RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW);
        return 0;
    }
    case progress_message: {
        std::unique_ptr<std::pair<std::uint64_t, std::uint64_t>> value(
            reinterpret_cast<std::pair<std::uint64_t, std::uint64_t>*>(lparam));
        if (value && value->second) {
            progress_total_ = value->second;
            progress_received_ = std::min(value->first, value->second);
            const int percent = static_cast<int>(std::min<std::uint64_t>(
                100, progress_received_ * 100 / progress_total_));
            set_state_message(
                L"已下载 " + format_size(progress_received_) + L" / " +
                format_size(progress_total_) + L"（" + std::to_wstring(percent) +
                L"%），完成后自动校验大小和 SHA-256。");
        }
        return 0;
    }
    case download_completed_message: {
        std::unique_ptr<DownloadResult> result(reinterpret_cast<DownloadResult*>(lparam));
        finish_worker();
        EnableWindow(action_button_, TRUE);
        EnableWindow(release_button_, TRUE);
        EnableWindow(check_button_, TRUE);
        if (!result) return 0;
        if (result->cancelled) {
            progress_visible_ = false;
            set_state(ViewState::available, L"下载已取消",
                      L"没有安装任何文件，可以随时重新下载。");
            action_role_ = ButtonRole::primary;
            SetWindowTextW(action_button_, L"下载并安装");
            resize_for_content();
        } else if (!result->error.empty()) {
            progress_visible_ = false;
            downloaded_.reset();
            set_state(ViewState::error, L"下载失败", result->error);
            action_role_ = ButtonRole::primary;
            SetWindowTextW(action_button_, L"重试下载");
            resize_for_content();
        } else {
            downloaded_ = std::move(result->path);
            progress_received_ = progress_total_;
            set_state(ViewState::ready, L"安装包已验证",
                      L"文件大小和 SHA-256 均与更新清单一致，可以开始安装。");
            verification_value_ = L"大小和 SHA-256 校验已通过";
            action_role_ = ButtonRole::primary;
            SetWindowTextW(action_button_, L"安装更新");
            pending_install_prompt_ = true;
            PostMessageW(window_, install_prompt_message, 0, 0);
        }
        layout_controls();
        RedrawWindow(window_, nullptr, nullptr,
                     RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW);
        return 0;
    }
    case install_prompt_message:
        if (pending_install_prompt_ && downloaded_ && available_) {
            pending_install_prompt_ = false;
            install();
        }
        return 0;
    case WM_DRAWITEM:
        if (const auto* item = reinterpret_cast<DRAWITEMSTRUCT*>(lparam)) {
            return draw_item(*item);
        }
        return FALSE;
    case WM_CTLCOLOREDIT:
    case WM_CTLCOLORSTATIC: {
        const HWND control = reinterpret_cast<HWND>(lparam);
        if (control == state_message_control_ ||
            control == published_value_control_ ||
            control == artifact_value_control_ ||
            control == source_control_) {
            HDC dc = reinterpret_cast<HDC>(wparam);
            const bool source = control == source_control_;
            const bool dim = source || control == state_message_control_;
            SetTextColor(dc, dim ? color_text_dim : color_text);
            SetBkColor(dc, source ? color_page : color_card);
            SetBkMode(dc, OPAQUE);
            return reinterpret_cast<LRESULT>(source ? background_ : card_background_);
        }
        break;
    }
    case WM_PAINT:
        paint();
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_KEYDOWN:
        if (wparam == VK_ESCAPE) {
            close();
            return 0;
        }
        break;
    case WM_CLOSE:
        close();
        return 0;
    case WM_NCDESTROY: {
        const HWND destroyed = window_;
        pending_install_prompt_ = false;
        if (worker_.joinable()) worker_.request_stop();
        invalidate_worker_dispatch();
        discard_worker_messages(destroyed);
        SetWindowLongPtrW(destroyed, GWLP_USERDATA, 0);
        window_ = nullptr;
        check_button_ = nullptr;
        release_button_ = nullptr;
        action_button_ = nullptr;
        state_message_control_ = nullptr;
        published_value_control_ = nullptr;
        artifact_value_control_ = nullptr;
        source_control_ = nullptr;
        repository_tooltip_ = nullptr;
        worker_dispatch_.reset();
        return DefWindowProcW(destroyed, message, wparam, lparam);
    }
    default:
        break;
    }
    return DefWindowProcW(window_, message, wparam, lparam);
}

}  // namespace screentrans
