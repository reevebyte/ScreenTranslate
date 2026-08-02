#include "selection_overlay.hpp"

#include "util.hpp"

#include <windowsx.h>
#include <wingdi.h>
#include <shellscalingapi.h>

#include <algorithm>
#include <array>
#include <cwchar>

namespace screentrans {

namespace {

constexpr wchar_t class_name[] = L"ScreenTranslate.Native.SelectionOverlay.v1";
constexpr int minimum_selection = 6;
constexpr int crosshair_half_length = 12;

int px(int value, int dpi) noexcept {
    return MulDiv(value, dpi, 96);
}

int dpi_at_point(POINT point) noexcept {
    const HMONITOR monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
    UINT dpi_x = 96;
    UINT dpi_y = 96;
    if (!monitor || FAILED(GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &dpi_x, &dpi_y))) {
        return 96;
    }
    return static_cast<int>(dpi_x);
}

COLORREF lighter(COLORREF color, int factor) noexcept {
    const auto channel = [factor](BYTE value) {
        return static_cast<BYTE>(std::clamp(
            static_cast<int>(value) * factor / 100,
            0, 255));
    };
    return RGB(channel(GetRValue(color)), channel(GetGValue(color)), channel(GetBValue(color)));
}

void dim_rectangle(HDC destination, HDC source, const RECT& rect) {
    if (rect.right <= rect.left || rect.bottom <= rect.top) {
        return;
    }
    SetPixelV(source, 0, 0, RGB(0, 0, 0));
    BLENDFUNCTION blend{AC_SRC_OVER, 0, 108, 0};
    AlphaBlend(destination, rect.left, rect.top,
               rect.right - rect.left, rect.bottom - rect.top,
               source, 0, 0, 1, 1, blend);
}

void alpha_fill(HDC destination, HDC source, const RECT& rect,
                COLORREF color, BYTE alpha) {
    if (!destination || !source || rect.right <= rect.left
        || rect.bottom <= rect.top || alpha == 0) return;
    SetPixelV(source, 0, 0, color);
    BLENDFUNCTION blend{AC_SRC_OVER, 0, alpha, 0};
    AlphaBlend(destination, rect.left, rect.top, rect.right - rect.left,
               rect.bottom - rect.top, source, 0, 0, 1, 1, blend);
}

void alpha_fill_region(HDC destination, HDC source, HRGN region,
                       const RECT& bounds,
                       COLORREF color, BYTE alpha) {
    if (!destination || !region || alpha == 0 || bounds.right <= bounds.left
        || bounds.bottom <= bounds.top) {
        return;
    }
    const int saved = SaveDC(destination);
    if (saved == 0) return;
    ExtSelectClipRgn(destination, region, RGN_AND);
    alpha_fill(destination, source, bounds, color, alpha);
    RestoreDC(destination, saved);
}

void alpha_frame(HDC destination, HDC source, const RECT& bounds, int thickness,
                 COLORREF color, BYTE alpha) {
    const int width = bounds.right - bounds.left;
    const int height = bounds.bottom - bounds.top;
    if (width <= 0 || height <= 0) return;
    thickness = std::clamp(thickness, 1, std::max(1, std::min(width, height) / 2));
    HRGN frame = CreateRectRgn(bounds.left, bounds.top, bounds.right, bounds.bottom);
    if (!frame) return;
    if (width > thickness * 2 && height > thickness * 2) {
        HRGN middle = CreateRectRgn(
            bounds.left + thickness, bounds.top + thickness,
            bounds.right - thickness, bounds.bottom - thickness);
        if (middle) {
            CombineRgn(frame, frame, middle, RGN_DIFF);
            DeleteObject(middle);
        }
    }
    alpha_fill_region(destination, source, frame, bounds, color, alpha);
    DeleteObject(frame);
}

void draw_handle(HDC dc, HDC alpha_dc, int center_x, int center_y,
                 int size, COLORREF accent) {
    const int half = size / 2;
    RECT rect{center_x - half, center_y - half,
              center_x - half + size, center_y - half + size};
    const int ellipse = std::max(2, size / 2);
    HRGN outer = CreateRoundRectRgn(rect.left, rect.top, rect.right, rect.bottom,
                                    ellipse, ellipse);
    if (!outer) return;
    alpha_fill_region(dc, alpha_dc, outer, rect, accent, 255);

    HRGN edge = CreateRoundRectRgn(rect.left, rect.top, rect.right, rect.bottom,
                                   ellipse, ellipse);
    if (edge && size > 2) {
        HRGN middle = CreateRoundRectRgn(rect.left + 1, rect.top + 1,
                                         rect.right - 1, rect.bottom - 1,
                                         std::max(2, ellipse - 2),
                                         std::max(2, ellipse - 2));
        if (middle) {
            CombineRgn(edge, edge, middle, RGN_DIFF);
            DeleteObject(middle);
        }
    }
    if (edge) {
        alpha_fill_region(dc, alpha_dc, edge, rect, RGB(255, 255, 255), 215);
        DeleteObject(edge);
    }
    DeleteObject(outer);
}

void draw_selection_frame(HDC dc, HDC alpha_dc, const RECT& selected,
                          COLORREF accent, int dpi) {
    const int width = selected.right - selected.left;
    const int height = selected.bottom - selected.top;
    if (width <= 0 || height <= 0) return;

    const int border = std::max(1, px(2, dpi));
    const int dark_border = std::max(1, px(1, dpi));
    alpha_frame(dc, alpha_dc, selected, dark_border, RGB(0, 0, 0), 90);
    RECT inner_dark{selected.left + border, selected.top + border,
                    selected.right - border, selected.bottom - border};
    if (inner_dark.right > inner_dark.left && inner_dark.bottom > inner_dark.top) {
        alpha_frame(dc, alpha_dc, inner_dark, dark_border, RGB(0, 0, 0), 90);
    }
    alpha_frame(dc, alpha_dc, selected, border, accent, 255);

    if (width < px(56, dpi) || height < px(34, dpi)) return;
    const int handle = std::max(px(8, dpi), 4);
    const int left = selected.left + handle / 2;
    const int right = selected.right - handle / 2;
    const int top = selected.top + handle / 2;
    const int bottom = selected.bottom - handle / 2;
    const int middle_x = (left + right) / 2;
    const int middle_y = (top + bottom) / 2;
    for (const POINT point : std::array<POINT, 8>{
             POINT{left, top}, POINT{middle_x, top}, POINT{right, top},
             POINT{left, middle_y}, POINT{right, middle_y},
             POINT{left, bottom}, POINT{middle_x, bottom}, POINT{right, bottom}}) {
        draw_handle(dc, alpha_dc, point.x, point.y, handle, accent);
    }
}

void draw_size_badge(HDC dc, HDC alpha_dc, const RECT& selected,
                     const RECT& bounds,
                     COLORREF accent, int dpi) {
    const int logical_width = std::max(1, MulDiv(selected.right - selected.left, 96, dpi));
    const int logical_height = std::max(1, MulDiv(selected.bottom - selected.top, 96, dpi));
    const std::wstring text = std::to_wstring(logical_width) + L" × " +
                              std::to_wstring(logical_height);
    HFONT font = CreateFontW(-px(11, dpi), 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                             DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                             CLEARTYPE_QUALITY, DEFAULT_PITCH, L"Microsoft YaHei UI");
    const auto old_font = SelectObject(dc, font);
    SIZE measured{};
    GetTextExtentPoint32W(dc, text.c_str(), static_cast<int>(text.size()), &measured);
    const int width = measured.cx + px(14, dpi);
    const int height = measured.cy + px(7, dpi);
    int left = std::clamp(selected.left, bounds.left, std::max(bounds.left, bounds.right - width));
    LONG top = selected.top - height - px(6, dpi);
    if (top < bounds.top) top = selected.top + px(6, dpi);
    top = std::clamp(top, bounds.top, std::max(bounds.top, bounds.bottom - height));
    RECT badge{left, top, left + width, top + height};

    HRGN badge_region = CreateRoundRectRgn(
        badge.left, badge.top, badge.right, badge.bottom, height, height);
    if (badge_region) {
        alpha_fill_region(dc, alpha_dc, badge_region, badge,
                          RGB(18, 20, 24), 232);
        DeleteObject(badge_region);
    }

    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, lighter(accent, 118));
    DrawTextW(dc, text.c_str(), static_cast<int>(text.size()), &badge,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, old_font);
    DeleteObject(font);
}

BOOL CALLBACK collect_monitor_rect(HMONITOR monitor, HDC, LPRECT, LPARAM data) {
    auto* rects = reinterpret_cast<std::vector<RECT>*>(data);
    MONITORINFO info{};
    info.cbSize = sizeof(info);
    if (GetMonitorInfoW(monitor, &info)
        && info.rcMonitor.right > info.rcMonitor.left
        && info.rcMonitor.bottom > info.rcMonitor.top) {
        rects->push_back(info.rcMonitor);
    }
    return TRUE;
}

std::vector<RECT> physical_monitor_rects() {
    std::vector<RECT> result;
    if (!EnumDisplayMonitors(nullptr, nullptr, &collect_monitor_rect,
                             reinterpret_cast<LPARAM>(&result))) {
        throw_last_error("enumerate displays");
    }
    return result;
}

}  // namespace

struct SelectionOverlay::WindowState {
    SelectionOverlay* owner{};
    HWND window{};
    RECT bounds{};
    POINT origin{};
    POINT current{};
    HDC paint_dc{};
    HBITMAP paint_bitmap{};
    HGDIOBJ paint_old_bitmap{};
    HDC alpha_dc{};
    HBITMAP alpha_bitmap{};
    HGDIOBJ alpha_old_bitmap{};
    int paint_width{};
    int paint_height{};
    UINT dpi{96};
    bool dragging{};
    bool mouse_inside{};
    bool tracking_mouse{};
};

SelectionOverlay::SelectionOverlay(HINSTANCE instance) : instance_(instance) {}

SelectionOverlay::~SelectionOverlay() {
    close();
}

bool SelectionOverlay::start(std::wstring_view accent,
                             SelectedCallback selected,
                             CancelledCallback cancelled) {
    if (active()) {
        return false;
    }

    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.lpfnWndProc = &SelectionOverlay::window_proc;
    description.hInstance = instance_;
    description.hCursor = nullptr;
    description.hbrBackground = nullptr;
    description.lpszClassName = class_name;
    const ATOM atom = RegisterClassExW(&description);
    if (!atom && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register selection overlay");
    }

    desktop_ = capture_virtual_desktop();
    accent_ = parse_rgb_color(accent, accent_);
    selected_ = std::move(selected);
    cancelled_ = std::move(cancelled);
    finishing_ = false;

    POINT cursor{};
    GetCursorPos(&cursor);
    WindowState* focus_target = nullptr;
    try {
        const auto monitors = physical_monitor_rects();
        windows_.reserve(monitors.size());
        for (const RECT& monitor : monitors) {
            RECT bounds{};
            if (!IntersectRect(&bounds, &monitor, &desktop_.bounds)
                || bounds.right <= bounds.left || bounds.bottom <= bounds.top) {
                continue;
            }

            auto state = std::make_unique<WindowState>();
            state->owner = this;
            state->bounds = bounds;
            state->current = POINT{cursor.x - bounds.left, cursor.y - bounds.top};
            state->mouse_inside = PtInRect(&bounds, cursor) != FALSE;

            const int width = bounds.right - bounds.left;
            const int height = bounds.bottom - bounds.top;
            const HWND window = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name, L"", WS_POPUP,
                bounds.left, bounds.top, width, height,
                nullptr, nullptr, instance_, state.get());
            if (!window) {
                throw_last_error("create selection overlay");
            }
            state->dpi = GetDpiForWindow(window);
            if (state->dpi == 0) {
                POINT center{bounds.left + width / 2, bounds.top + height / 2};
                state->dpi = static_cast<UINT>(dpi_at_point(center));
            }
            if (state->mouse_inside) focus_target = state.get();
            windows_.push_back(std::move(state));
        }
        if (windows_.empty()) {
            throw AppError("no active displays are available for selection");
        }
    } catch (...) {
        close();
        throw;
    }

    for (const auto& state : windows_) {
        const int width = state->bounds.right - state->bounds.left;
        const int height = state->bounds.bottom - state->bounds.top;
        ShowWindow(state->window, SW_SHOWNOACTIVATE);
        SetWindowPos(state->window, HWND_TOPMOST,
                     state->bounds.left, state->bounds.top, width, height,
                     SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    if (!focus_target) focus_target = windows_.front().get();
    if (focus_target && focus_target->window) {
        SetForegroundWindow(focus_target->window);
        SetFocus(focus_target->window);
    }
    return true;
}

void SelectionOverlay::close() {
    std::vector<std::unique_ptr<WindowState>> windows;
    windows.swap(windows_);
    for (const auto& state : windows) {
        if (state->window && IsWindow(state->window)) {
            DestroyWindow(state->window);
        }
        release_paint_resources(*state);
    }
    desktop_.pixels = {};
    selected_ = {};
    cancelled_ = {};
    finishing_ = false;
}

LRESULT CALLBACK SelectionOverlay::window_proc(HWND window, UINT message,
                                                WPARAM wparam, LPARAM lparam) {
    auto* state = reinterpret_cast<WindowState*>(
        GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
        state = static_cast<WindowState*>(create->lpCreateParams);
        state->window = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(state));
    }
    if (!state || !state->owner) {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    return state->owner->handle_message(*state, message, wparam, lparam);
}

LRESULT SelectionOverlay::handle_message(WindowState& state, UINT message,
                                          WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT:
        paint(state);
        return 0;
    case WM_SETCURSOR:
        SetCursor(nullptr);
        return TRUE;
    case WM_MOUSEMOVE: {
        state.current = local_cursor(state);
        state.mouse_inside = true;
        if (!state.tracking_mouse && !state.dragging) {
            TRACKMOUSEEVENT tracking{};
            tracking.cbSize = sizeof(tracking);
            tracking.dwFlags = TME_LEAVE;
            tracking.hwndTrack = state.window;
            state.tracking_mouse = TrackMouseEvent(&tracking) != FALSE;
        }
        InvalidateRect(state.window, nullptr, FALSE);
        return 0;
    }
    case WM_MOUSELEAVE:
        state.tracking_mouse = false;
        if (!state.dragging) {
            state.mouse_inside = false;
            InvalidateRect(state.window, nullptr, FALSE);
        }
        return 0;
    case WM_LBUTTONDOWN:
        if (finishing_) return 0;
        SetForegroundWindow(state.window);
        SetFocus(state.window);
        state.origin = state.current = local_cursor(state);
        state.dragging = true;
        SetCapture(state.window);
        InvalidateRect(state.window, nullptr, FALSE);
        return 0;
    case WM_LBUTTONUP:
        if (state.dragging) {
            state.current = local_cursor(state);
            if (GetCapture() == state.window) ReleaseCapture();
            finish_selection(state);
        }
        return 0;
    case WM_RBUTTONDOWN:
        cancel();
        return 0;
    case WM_KEYDOWN:
        if (wparam == VK_ESCAPE || wparam == L'Q') {
            cancel();
            return 0;
        }
        break;
    case WM_DPICHANGED: {
        state.dpi = HIWORD(wparam);
        const int width = state.bounds.right - state.bounds.left;
        const int height = state.bounds.bottom - state.bounds.top;
        SetWindowPos(state.window, HWND_TOPMOST,
                     state.bounds.left, state.bounds.top, width, height,
                     SWP_NOACTIVATE | SWP_SHOWWINDOW);
        InvalidateRect(state.window, nullptr, FALSE);
        return 0;
    }
    case WM_NCDESTROY: {
        const HWND destroyed = state.window;
        SetWindowLongPtrW(destroyed, GWLP_USERDATA, 0);
        state.window = nullptr;
        state.tracking_mouse = false;
        release_paint_resources(state);
        return DefWindowProcW(destroyed, message, wparam, lparam);
    }
    default:
        break;
    }
    return DefWindowProcW(state.window, message, wparam, lparam);
}

POINT SelectionOverlay::local_cursor(const WindowState& state) const {
    POINT point{};
    GetCursorPos(&point);
    point.x -= state.bounds.left;
    point.y -= state.bounds.top;
    return point;
}

RECT SelectionOverlay::selection_rect(const WindowState& state) const {
    const LONG width = state.bounds.right - state.bounds.left;
    const LONG height = state.bounds.bottom - state.bounds.top;
    RECT value{
        std::min(state.origin.x, state.current.x),
        std::min(state.origin.y, state.current.y),
        std::max(state.origin.x, state.current.x) + 1,
        std::max(state.origin.y, state.current.y) + 1,
    };
    value.left = std::clamp(value.left, 0L, width);
    value.top = std::clamp(value.top, 0L, height);
    value.right = std::clamp(value.right, 0L, width);
    value.bottom = std::clamp(value.bottom, 0L, height);
    return value;
}

void SelectionOverlay::finish_selection(WindowState& state) {
    if (finishing_) {
        return;
    }
    state.dragging = false;
    const RECT local = selection_rect(state);
    const int minimum = std::max(1, px(minimum_selection, static_cast<int>(state.dpi)));
    if (local.right - local.left < minimum || local.bottom - local.top < minimum) {
        cancel();
        return;
    }
    finishing_ = true;
    RECT physical{
        local.left + state.bounds.left,
        local.top + state.bounds.top,
        local.right + state.bounds.left,
        local.bottom + state.bounds.top,
    };
    const int source_left = physical.left - desktop_.bounds.left;
    const int source_top = physical.top - desktop_.bounds.top;
    auto crop = desktop_.pixels.crop(source_left, source_top,
                                     local.right - local.left,
                                     local.bottom - local.top);
    for (const auto& overlay : windows_) {
        if (!overlay->window) continue;
        const LONG_PTR styles = GetWindowLongPtrW(overlay->window, GWL_EXSTYLE);
        SetWindowLongPtrW(overlay->window, GWL_EXSTYLE,
                          styles | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE);
    }
    // This callback is deliberately synchronous. AppHost creates and presents
    // the result surface while every frozen monitor overlay still covers the
    // desktop.
    // Closing first would expose a live desktop frame and reintroduce the flash.
    auto callback = selected_;
    try {
        if (callback) callback(physical, std::move(crop));
    } catch (...) {
        OutputDebugStringW(L"ScreenTranslate: selection callback failed.\n");
        close();
        return;
    }
    close();
}

void SelectionOverlay::cancel() {
    if (finishing_) {
        return;
    }
    finishing_ = true;
    auto callback = cancelled_;
    close();
    if (callback) {
        callback();
    }
}

bool SelectionOverlay::ensure_paint_resources(WindowState& state, HDC compatible,
                                               int width, int height) {
    if (state.paint_dc && state.paint_bitmap && state.alpha_dc && state.alpha_bitmap
        && state.paint_width == width && state.paint_height == height) {
        return true;
    }
    release_paint_resources(state);

    state.paint_dc = CreateCompatibleDC(compatible);
    state.paint_bitmap = state.paint_dc
        ? CreateCompatibleBitmap(compatible, width, height) : nullptr;
    if (state.paint_dc && state.paint_bitmap) {
        state.paint_old_bitmap = SelectObject(state.paint_dc, state.paint_bitmap);
    }
    state.alpha_dc = CreateCompatibleDC(compatible);
    state.alpha_bitmap = state.alpha_dc
        ? CreateCompatibleBitmap(compatible, 1, 1) : nullptr;
    if (state.alpha_dc && state.alpha_bitmap) {
        state.alpha_old_bitmap = SelectObject(state.alpha_dc, state.alpha_bitmap);
    }
    if (!state.paint_dc || !state.paint_bitmap || !state.paint_old_bitmap
        || state.paint_old_bitmap == HGDI_ERROR || !state.alpha_dc
        || !state.alpha_bitmap || !state.alpha_old_bitmap
        || state.alpha_old_bitmap == HGDI_ERROR) {
        release_paint_resources(state);
        return false;
    }
    state.paint_width = width;
    state.paint_height = height;
    return true;
}

void SelectionOverlay::release_paint_resources(WindowState& state) noexcept {
    if (state.paint_dc && state.paint_old_bitmap
        && state.paint_old_bitmap != HGDI_ERROR) {
        SelectObject(state.paint_dc, state.paint_old_bitmap);
    }
    if (state.alpha_dc && state.alpha_old_bitmap
        && state.alpha_old_bitmap != HGDI_ERROR) {
        SelectObject(state.alpha_dc, state.alpha_old_bitmap);
    }
    if (state.paint_bitmap) DeleteObject(state.paint_bitmap);
    if (state.alpha_bitmap) DeleteObject(state.alpha_bitmap);
    if (state.paint_dc) DeleteDC(state.paint_dc);
    if (state.alpha_dc) DeleteDC(state.alpha_dc);
    state.paint_dc = nullptr;
    state.paint_bitmap = nullptr;
    state.paint_old_bitmap = nullptr;
    state.alpha_dc = nullptr;
    state.alpha_bitmap = nullptr;
    state.alpha_old_bitmap = nullptr;
    state.paint_width = 0;
    state.paint_height = 0;
}

void SelectionOverlay::paint(WindowState& state) {
    PAINTSTRUCT paint_state{};
    HDC target = BeginPaint(state.window, &paint_state);
    if (!target) return;
    const auto& image = desktop_.pixels;
    RECT client{};
    GetClientRect(state.window, &client);
    if (client.right <= client.left || client.bottom <= client.top) {
        EndPaint(state.window, &paint_state);
        return;
    }

    if (!ensure_paint_resources(state, target, client.right, client.bottom)) {
        EndPaint(state.window, &paint_state);
        return;
    }
    HDC dc = state.paint_dc;

    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = image.width;
    info.bmiHeader.biHeight = -image.height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    const int source_left = state.bounds.left - desktop_.bounds.left;
    const int source_top = state.bounds.top - desktop_.bounds.top;
    const int source_width = state.bounds.right - state.bounds.left;
    const int source_height = state.bounds.bottom - state.bounds.top;
    StretchDIBits(dc, 0, 0, client.right, client.bottom,
                  source_left, source_top, source_width, source_height,
                  image.bgra.data(), &info, DIB_RGB_COLORS, SRCCOPY);

    HDC dim_source = state.alpha_dc;

    const int dpi = static_cast<int>(state.dpi);
    if (!state.dragging) {
        dim_rectangle(dc, dim_source, client);
        if (state.mouse_inside && state.current.x >= client.left
            && state.current.x < client.right && state.current.y >= client.top
            && state.current.y < client.bottom) {
            const int half = px(crosshair_half_length, dpi);
            const int stroke = std::max(1, px(2, dpi));
            const int stroke_offset = stroke / 2;
            RECT horizontal{
                std::max(client.left, state.current.x - half),
                std::max(client.top, state.current.y - stroke_offset),
                std::min(client.right, state.current.x + half + 1),
                std::min(client.bottom, state.current.y - stroke_offset + stroke),
            };
            RECT vertical{
                std::max(client.left, state.current.x - stroke_offset),
                std::max(client.top, state.current.y - half),
                std::min(client.right, state.current.x - stroke_offset + stroke),
                std::min(client.bottom, state.current.y + half + 1),
            };
            alpha_fill(dc, dim_source, horizontal, accent_, 235);
            alpha_fill(dc, dim_source, vertical, accent_, 235);
        }
    } else {
        const RECT selected = selection_rect(state);
        dim_rectangle(dc, dim_source,
                      RECT{client.left, client.top, client.right, selected.top});
        dim_rectangle(dc, dim_source,
                      RECT{client.left, selected.bottom, client.right, client.bottom});
        dim_rectangle(dc, dim_source,
                      RECT{client.left, selected.top, selected.left, selected.bottom});
        dim_rectangle(dc, dim_source,
                      RECT{selected.right, selected.top, client.right, selected.bottom});
        draw_selection_frame(dc, dim_source, selected, accent_, dpi);
        draw_size_badge(dc, dim_source, selected, client, accent_, dpi);
    }

    BitBlt(target, 0, 0, client.right, client.bottom, dc, 0, 0, SRCCOPY);
    EndPaint(state.window, &paint_state);
}

}  // namespace screentrans
