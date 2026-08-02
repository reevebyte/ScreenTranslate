#include "result_window.hpp"

#include "capture.hpp"
#include "util.hpp"

#include <commctrl.h>
#include <d2d1.h>
#include <dwmapi.h>
#include <uxtheme.h>
#include <windowsx.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <utility>

namespace screentrans {

namespace {

constexpr wchar_t class_name[] = L"ScreenTranslate.Native.ResultWindow.v1";
constexpr wchar_t control_bar_class_name[] = L"ScreenTranslate.Native.ResultControlBar.v1";
// Keep the detached result toolbar dimensionally identical to the Python
// reference.  At 26 px the native toolbar covered noticeably more of the
// surrounding desktop and made the glyphs read heavier than the original.
constexpr int logical_button_size = 20;
constexpr int logical_bar_padding = 4;
constexpr int logical_bar_gap = 5;
constexpr int logical_grip_width = 10;
constexpr int logical_button_spacing = 2;
constexpr int logical_border_hit = 8;
constexpr UINT_PTR close_timer = 3;
constexpr UINT_PTR leave_timer = 4;
constexpr UINT_PTR progress_timer = 5;
constexpr UINT_PTR recapture_restore_timer = 6;
constexpr COLORREF resize_transparent_key = RGB(1, 2, 3);
constexpr std::array<int, 4> command_ids{4103, 4102, 4104, 4105};
constexpr std::array<const wchar_t*, 4> button_tips{
    L"校对识别文字（E）", L"用当前接口重新翻译（R）",
    L"缩到托盘（M）", L"关闭（Esc）",
};

int dpi_value(HWND window, int logical) {
    const UINT dpi = window ? GetDpiForWindow(window) : 96;
    return std::max(1, MulDiv(logical, static_cast<int>(dpi ? dpi : 96), 96));
}

bool equal_rect(const RECT& left, const RECT& right) {
    return left.left == right.left && left.top == right.top
        && left.right == right.right && left.bottom == right.bottom;
}

bool equal_size(const RECT& left, const RECT& right) {
    return left.right - left.left == right.right - right.left
        && left.bottom - left.top == right.bottom - right.top;
}

void alpha_fill(HDC destination, const RECT& rect, COLORREF color, BYTE alpha) {
    if (!destination || rect.right <= rect.left || rect.bottom <= rect.top || alpha == 0) return;
    HDC source = CreateCompatibleDC(destination);
    HBITMAP bitmap = CreateCompatibleBitmap(destination, 1, 1);
    if (!source || !bitmap) {
        if (bitmap) DeleteObject(bitmap);
        if (source) DeleteDC(source);
        return;
    }
    const HGDIOBJ old = SelectObject(source, bitmap);
    SetPixelV(source, 0, 0, color);
    BLENDFUNCTION blend{AC_SRC_OVER, 0, alpha, 0};
    AlphaBlend(destination, rect.left, rect.top, rect.right - rect.left,
               rect.bottom - rect.top, source, 0, 0, 1, 1, blend);
    SelectObject(source, old);
    DeleteObject(bitmap);
    DeleteDC(source);
}

void alpha_fill_region(HDC destination, HRGN region, const RECT& bounds,
                       COLORREF color, BYTE alpha) {
    if (!destination || !region || alpha == 0 || bounds.right <= bounds.left
        || bounds.bottom <= bounds.top) {
        return;
    }
    const int saved = SaveDC(destination);
    if (saved == 0) return;
    ExtSelectClipRgn(destination, region, RGN_AND);
    alpha_fill(destination, bounds, color, alpha);
    RestoreDC(destination, saved);
}

void alpha_frame(HDC destination, const RECT& bounds, int thickness,
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
    alpha_fill_region(destination, frame, bounds, color, alpha);
    DeleteObject(frame);
}

void draw_alpha_handle(HDC destination, const RECT& bounds, COLORREF accent,
                       double opacity, int corner_radius) {
    const double clamped = std::clamp(opacity, 0.0, 1.0);
    if (clamped <= 0.0 || bounds.right <= bounds.left || bounds.bottom <= bounds.top) return;
    const int ellipse = std::max(2, corner_radius * 2);
    HRGN outer = CreateRoundRectRgn(bounds.left, bounds.top, bounds.right, bounds.bottom,
                                    ellipse, ellipse);
    if (!outer) return;
    alpha_fill_region(destination, outer, bounds, accent,
                      static_cast<BYTE>(std::lround(clamped * 255.0)));

    HRGN edge = CreateRoundRectRgn(bounds.left, bounds.top, bounds.right, bounds.bottom,
                                   ellipse, ellipse);
    if (edge && bounds.right - bounds.left > 2 && bounds.bottom - bounds.top > 2) {
        HRGN middle = CreateRoundRectRgn(
            bounds.left + 1, bounds.top + 1, bounds.right - 1, bounds.bottom - 1,
            std::max(2, ellipse - 2), std::max(2, ellipse - 2));
        if (middle) {
            CombineRgn(edge, edge, middle, RGN_DIFF);
            DeleteObject(middle);
        }
    }
    if (edge) {
        alpha_fill_region(destination, edge, bounds, RGB(255, 255, 255),
                          static_cast<BYTE>(std::lround(clamped * 215.0)));
        DeleteObject(edge);
    }
    DeleteObject(outer);
}

ID2D1Factory* d2d_factory() noexcept {
    static ID2D1Factory* factory = [] {
        ID2D1Factory* value = nullptr;
        D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, &value);
        return value;
    }();
    return factory;
}

ID2D1StrokeStyle* round_stroke_style() noexcept {
    static ID2D1StrokeStyle* style = [] {
        auto* factory = d2d_factory();
        if (!factory) return static_cast<ID2D1StrokeStyle*>(nullptr);
        D2D1_STROKE_STYLE_PROPERTIES properties{};
        properties.startCap = D2D1_CAP_STYLE_ROUND;
        properties.endCap = D2D1_CAP_STYLE_ROUND;
        properties.dashCap = D2D1_CAP_STYLE_ROUND;
        properties.lineJoin = D2D1_LINE_JOIN_ROUND;
        properties.miterLimit = 2.0F;
        properties.dashStyle = D2D1_DASH_STYLE_SOLID;
        ID2D1StrokeStyle* value = nullptr;
        factory->CreateStrokeStyle(properties, nullptr, 0, &value);
        return value;
    }();
    return style;
}

D2D1_COLOR_F d2d_color(COLORREF color, float alpha = 1.0F) noexcept {
    return D2D1::ColorF(
        static_cast<float>(GetRValue(color)) / 255.0F,
        static_cast<float>(GetGValue(color)) / 255.0F,
        static_cast<float>(GetBValue(color)) / 255.0F,
        std::clamp(alpha, 0.0F, 1.0F));
}

void draw_control_icon(ID2D1RenderTarget* target, int index,
                       const D2D1_RECT_F& bounds, COLORREF color,
                       float opacity = 1.0F) {
    if (!target || index < 0 || index >= 4) return;
    ID2D1SolidColorBrush* brush = nullptr;
    if (FAILED(target->CreateSolidColorBrush(d2d_color(color, opacity), &brush))) return;

    const float scale = std::min(bounds.right - bounds.left,
                                 bounds.bottom - bounds.top) / 24.0F;
    if (scale <= 0.0F) {
        brush->Release();
        return;
    }
    const float origin_x = (bounds.left + bounds.right) * 0.5F - 12.0F * scale;
    const float origin_y = (bounds.top + bounds.bottom) * 0.5F - 12.0F * scale;
    const auto point = [&](float x, float y) {
        return D2D1::Point2F(origin_x + x * scale, origin_y + y * scale);
    };
    const float adjusted_weight = 2.1F *
        (scale >= 0.62F ? 1.0F : std::pow(0.62F / scale, 0.45F));
    const float stroke = adjusted_weight * scale;
    auto* stroke_style = round_stroke_style();
    const auto line = [&](float x1, float y1, float x2, float y2) {
        target->DrawLine(point(x1, y1), point(x2, y2), brush, stroke, stroke_style);
    };

    if (index == 0) {
        constexpr std::array<D2D1_POINT_2F, 5> body{{
            {5.2F, 15.4F}, {14.8F, 5.8F}, {18.2F, 9.2F},
            {8.6F, 18.8F}, {4.2F, 19.8F},
        }};
        for (std::size_t i = 1; i < body.size(); ++i) {
            line(body[i - 1].x, body[i - 1].y, body[i].x, body[i].y);
        }
        line(5.2F, 15.4F, 8.6F, 18.8F);
        line(13.3F, 7.3F, 16.7F, 10.7F);
    } else if (index == 1) {
        constexpr float radius = 6.4F;
        constexpr float radians = 105.0F * 3.14159265358979323846F / 180.0F;
        constexpr int arc_segments = 4;
        constexpr float segment_angle =
            75.0F * 3.14159265358979323846F / 180.0F;
        const float bezier_factor =
            4.0F / 3.0F * std::tan(segment_angle / 4.0F);
        const auto arc_point = [&](float angle) {
            return D2D1::Point2F(12.0F + radius * std::cos(angle),
                                12.0F - radius * std::sin(angle));
        };
        const auto start = arc_point(radians);
        ID2D1PathGeometry* geometry = nullptr;
        ID2D1GeometrySink* sink = nullptr;
        if (auto* factory = d2d_factory(); factory &&
            SUCCEEDED(factory->CreatePathGeometry(&geometry)) &&
            SUCCEEDED(geometry->Open(&sink))) {
            sink->BeginFigure(point(start.x, start.y), D2D1_FIGURE_BEGIN_HOLLOW);
            for (int segment = 0; segment < arc_segments; ++segment) {
                const float angle0 = radians + segment * segment_angle;
                const float angle1 = angle0 + segment_angle;
                const auto end = arc_point(angle1);
                const auto control1 = D2D1::Point2F(
                    12.0F + radius * std::cos(angle0)
                        - radius * std::sin(angle0) * bezier_factor,
                    12.0F - radius * std::sin(angle0)
                        - radius * std::cos(angle0) * bezier_factor);
                const auto control2 = D2D1::Point2F(
                    end.x + radius * std::sin(angle1) * bezier_factor,
                    end.y + radius * std::cos(angle1) * bezier_factor);
                sink->AddBezier(D2D1::BezierSegment(
                    point(control1.x, control1.y),
                    point(control2.x, control2.y),
                    point(end.x, end.y)));
            }
            sink->EndFigure(D2D1_FIGURE_END_OPEN);
            sink->Close();
            target->DrawGeometry(geometry, brush, stroke, stroke_style);
        }
        if (sink) sink->Release();
        if (geometry) geometry->Release();

        const float px = 12.0F + radius * std::cos(radians);
        const float py = 12.0F - radius * std::sin(radians);
        const float tx = std::sin(radians);
        const float ty = std::cos(radians);
        const float nx = -ty;
        const float ny = tx;
        ID2D1PathGeometry* arrow = nullptr;
        ID2D1GeometrySink* arrow_sink = nullptr;
        if (auto* factory = d2d_factory(); factory &&
            SUCCEEDED(factory->CreatePathGeometry(&arrow)) &&
            SUCCEEDED(arrow->Open(&arrow_sink))) {
            arrow_sink->BeginFigure(point(px + tx * 4.2F, py + ty * 4.2F),
                                    D2D1_FIGURE_BEGIN_FILLED);
            arrow_sink->AddLine(point(px - tx * 0.8F + nx * 2.9F,
                                      py - ty * 0.8F + ny * 2.9F));
            arrow_sink->AddLine(point(px - tx * 0.8F - nx * 2.9F,
                                      py - ty * 0.8F - ny * 2.9F));
            arrow_sink->EndFigure(D2D1_FIGURE_END_CLOSED);
            arrow_sink->Close();
            target->FillGeometry(arrow, brush);
        }
        if (arrow_sink) arrow_sink->Release();
        if (arrow) arrow->Release();
    } else if (index == 2) {
        line(7.0F, 12.0F, 17.0F, 12.0F);
    } else {
        line(7.5F, 7.5F, 16.5F, 16.5F);
        line(16.5F, 7.5F, 7.5F, 16.5F);
    }
    brush->Release();
}

struct SizeBadge {
    std::wstring text;
    RECT bounds{};
    int font_pixels{};
};

SizeBadge make_size_badge(HWND window, int width, int height) {
    const UINT dpi = GetDpiForWindow(window);
    const int safe_dpi = static_cast<int>(dpi ? dpi : 96);
    SizeBadge badge;
    badge.text = std::to_wstring(MulDiv(width, 96, safe_dpi)) + L" × "
               + std::to_wstring(MulDiv(height, 96, safe_dpi));
    badge.font_pixels = dpi_value(window, 11);
    SIZE measured{};
    HDC dc = GetDC(window);
    HFONT font = CreateFontW(-badge.font_pixels, 0, 0, 0, FW_NORMAL,
                             FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                             OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                             CLEARTYPE_QUALITY, DEFAULT_PITCH,
                             L"Microsoft YaHei UI");
    if (dc && font) {
        const HGDIOBJ old_font = SelectObject(dc, font);
        GetTextExtentPoint32W(dc, badge.text.c_str(),
                              static_cast<int>(badge.text.size()), &measured);
        SelectObject(dc, old_font);
    }
    if (font) DeleteObject(font);
    if (dc) ReleaseDC(window, dc);
    const int top = dpi_value(window, 6);
    badge.bounds = {
        0, top,
        std::min(width, static_cast<int>(measured.cx) + dpi_value(window, 14)),
        std::min(height, top + static_cast<int>(measured.cy) + dpi_value(window, 7)),
    };
    return badge;
}

}  // namespace

ResultWindow::ResultWindow(HINSTANCE instance) : instance_(instance) {}

ResultWindow::~ResultWindow() { close(); }

void ResultWindow::create_window() {
    if (window_) return;
    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.lpfnWndProc = &ResultWindow::window_proc;
    description.style = CS_DBLCLKS;
    description.hInstance = instance_;
    description.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    description.hbrBackground = nullptr;
    description.lpszClassName = class_name;
    const ATOM atom = RegisterClassExW(&description);
    if (!atom && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register result window");
    }
    window_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                              class_name, L"划词截屏翻译",
                              WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                              0, 0, 1, 1, nullptr, nullptr, instance_, this);
    if (!window_) throw_last_error("create result window");
    SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
    create_control_bar();
}

void ResultWindow::create_control_bar() {
    if (control_bar_) return;
    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.lpfnWndProc = &ResultWindow::control_bar_proc;
    description.style = 0;
    description.hInstance = instance_;
    description.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    description.hbrBackground = nullptr;
    description.lpszClassName = control_bar_class_name;
    const ATOM atom = RegisterClassExW(&description);
    if (!atom && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register result control bar");
    }
    control_bar_ = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
        control_bar_class_name, L"结果控制", WS_POPUP,
        0, 0, 1, 1, window_, nullptr, instance_, this);
    if (!control_bar_) throw_last_error("create result control bar");
    update_control_bar_region();

    control_bar_tooltip_ = CreateWindowExW(
        WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr,
        WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
        CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
        control_bar_, nullptr, instance_, nullptr);
    if (control_bar_tooltip_) {
        SetWindowTheme(control_bar_tooltip_, L"DarkMode_Explorer", nullptr);
        SendMessageW(control_bar_tooltip_, TTM_SETTIPBKCOLOR,
                     static_cast<WPARAM>(RGB(38, 40, 46)), 0);
        SendMessageW(control_bar_tooltip_, TTM_SETTIPTEXTCOLOR,
                     static_cast<WPARAM>(RGB(242, 243, 245)), 0);
        SetWindowPos(control_bar_tooltip_, HWND_TOPMOST, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        for (std::size_t index = 0; index < command_ids.size(); ++index) {
            TOOLINFOW tool{};
            tool.cbSize = TTTOOLINFOW_V2_SIZE;
            tool.uFlags = TTF_SUBCLASS | TTF_TRANSPARENT;
            tool.hwnd = control_bar_;
            tool.uId = static_cast<UINT_PTR>(index + 1);
            tool.rect = control_bar_button_rect(static_cast<int>(index));
            tool.lpszText = const_cast<wchar_t*>(button_tips[index]);
            SendMessageW(control_bar_tooltip_, TTM_ADDTOOLW, 0,
                         reinterpret_cast<LPARAM>(&tool));
        }
        RECT client{};
        GetClientRect(control_bar_, &client);
        TOOLINFOW grip_tool{};
        grip_tool.cbSize = TTTOOLINFOW_V2_SIZE;
        grip_tool.uFlags = TTF_SUBCLASS | TTF_TRANSPARENT;
        grip_tool.hwnd = control_bar_;
        grip_tool.uId = 100;
        grip_tool.rect = {0, 0, control_bar_button_rect(0).left, client.bottom};
        grip_tool.lpszText = const_cast<wchar_t*>(L"拖动这里可以移动窗口");
        SendMessageW(control_bar_tooltip_, TTM_ADDTOOLW, 0,
                     reinterpret_cast<LPARAM>(&grip_tool));
    }
}

void ResultWindow::destroy_control_bar() {
    if (control_bar_tooltip_) {
        DestroyWindow(control_bar_tooltip_);
        control_bar_tooltip_ = nullptr;
    }
    if (control_bar_) {
        const HWND old = control_bar_;
        DestroyWindow(old);
        if (control_bar_ == old) control_bar_ = nullptr;
    }
    control_bar_hover_ = -1;
    control_bar_pressed_ = -1;
    control_bar_dragging_ = false;
}

RECT ResultWindow::control_bar_button_rect(int index) const {
    const int padding = dpi_value(window_, logical_bar_padding);
    const int grip = dpi_value(window_, logical_grip_width);
    const int button = dpi_value(window_, logical_button_size);
    const int spacing = dpi_value(window_, logical_button_spacing);
    const int left = padding + grip + index * (button + spacing);
    return {left, padding, left + button, padding + button};
}

int ResultWindow::control_bar_button_at(POINT point) const {
    for (int index = 0; index < static_cast<int>(command_ids.size()); ++index) {
        const RECT bounds = control_bar_button_rect(index);
        if (PtInRect(&bounds, point)) return index;
    }
    return -1;
}

void ResultWindow::update_control_bar_region() {
    if (!control_bar_) return;
    const int padding = dpi_value(window_, logical_bar_padding);
    const int grip = dpi_value(window_, logical_grip_width);
    const int button = dpi_value(window_, logical_button_size);
    const int spacing = dpi_value(window_, logical_button_spacing);
    const int width = padding + grip + button * static_cast<int>(command_ids.size())
                    + spacing * (static_cast<int>(command_ids.size()) - 1) + padding;
    const int height = padding * 2 + button;
    SetWindowPos(control_bar_, nullptr, 0, 0, width, height,
                 SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    SetWindowRgn(control_bar_, nullptr, FALSE);

    if (control_bar_tooltip_) {
        for (std::size_t index = 0; index < command_ids.size(); ++index) {
            TOOLINFOW tool{};
            tool.cbSize = TTTOOLINFOW_V2_SIZE;
            tool.hwnd = control_bar_;
            tool.uId = static_cast<UINT_PTR>(index + 1);
            tool.rect = control_bar_button_rect(static_cast<int>(index));
            SendMessageW(control_bar_tooltip_, TTM_NEWTOOLRECTW, 0,
                         reinterpret_cast<LPARAM>(&tool));
        }
        RECT client{};
        GetClientRect(control_bar_, &client);
        TOOLINFOW grip_tool{};
        grip_tool.cbSize = TTTOOLINFOW_V2_SIZE;
        grip_tool.hwnd = control_bar_;
        grip_tool.uId = 100;
        grip_tool.rect = {0, 0, control_bar_button_rect(0).left, client.bottom};
        SendMessageW(control_bar_tooltip_, TTM_NEWTOOLRECTW, 0,
                     reinterpret_cast<LPARAM>(&grip_tool));
    }
}

void ResultWindow::sync_control_bar() {
    if (!window_ || !control_bar_ || !IsWindowVisible(window_)) return;
    RECT result_rect{};
    RECT bar_rect{};
    GetWindowRect(window_, &result_rect);
    GetWindowRect(control_bar_, &bar_rect);
    const int bar_width = bar_rect.right - bar_rect.left;
    const int bar_height = bar_rect.bottom - bar_rect.top;
    const int gap = dpi_value(window_, logical_bar_gap);

    MONITORINFO monitor{};
    monitor.cbSize = sizeof(monitor);
    GetMonitorInfoW(MonitorFromRect(&result_rect, MONITOR_DEFAULTTONEAREST), &monitor);
    int x = result_rect.right - bar_width;
    int y = result_rect.bottom + gap;
    if (y + bar_height > monitor.rcWork.bottom) {
        y = result_rect.bottom - bar_height - gap;
    }
    x = std::clamp(x, static_cast<int>(monitor.rcWork.left),
                   static_cast<int>(monitor.rcWork.right) - bar_width);
    y = std::max(static_cast<int>(monitor.rcWork.top), y);
    SetWindowPos(control_bar_, HWND_TOPMOST, x, y, bar_width, bar_height,
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

void ResultWindow::update_window_region() {
    if (!window_) return;
    // A changing fragmented HRGN exposes desktop holes while the DWM resize loop
    // is still presenting older frames. Keep one rectangular surface and repaint
    // it atomically instead.
    SetWindowRgn(window_, nullptr, FALSE);
}

HDC ResultWindow::ensure_paint_buffer(HDC target, int width, int height) {
    if (!target || width <= 0 || height <= 0) return nullptr;
    if (!paint_buffer_dc_) {
        paint_buffer_dc_ = CreateCompatibleDC(target);
        if (!paint_buffer_dc_) return nullptr;
    }
    if (paint_buffer_bitmap_ && width <= paint_buffer_width_ &&
        height <= paint_buffer_height_) {
        return paint_buffer_dc_;
    }

    const int grown_width = std::max(width, paint_buffer_width_ + paint_buffer_width_ / 2);
    const int grown_height = std::max(height, paint_buffer_height_ + paint_buffer_height_ / 2);
    HBITMAP replacement = CreateCompatibleBitmap(target, grown_width, grown_height);
    if (!replacement) return nullptr;
    const HGDIOBJ previous = SelectObject(paint_buffer_dc_, replacement);
    if (!previous || previous == HGDI_ERROR) {
        DeleteObject(replacement);
        return nullptr;
    }
    if (!paint_buffer_original_) paint_buffer_original_ = previous;
    if (paint_buffer_bitmap_) DeleteObject(paint_buffer_bitmap_);
    paint_buffer_bitmap_ = replacement;
    paint_buffer_width_ = grown_width;
    paint_buffer_height_ = grown_height;
    return paint_buffer_dc_;
}

void ResultWindow::release_paint_buffer() noexcept {
    if (paint_buffer_dc_ && paint_buffer_original_) {
        SelectObject(paint_buffer_dc_, paint_buffer_original_);
    }
    if (paint_buffer_bitmap_) DeleteObject(paint_buffer_bitmap_);
    if (paint_buffer_dc_) DeleteDC(paint_buffer_dc_);
    paint_buffer_dc_ = nullptr;
    paint_buffer_bitmap_ = nullptr;
    paint_buffer_original_ = nullptr;
    paint_buffer_width_ = 0;
    paint_buffer_height_ = 0;
}

void ResultWindow::paint_control_bar() {
    if (!control_bar_) return;
    PAINTSTRUCT state{};
    if (!BeginPaint(control_bar_, &state)) return;
    RECT client{};
    GetClientRect(control_bar_, &client);
    const int width = client.right - client.left;
    const int height = client.bottom - client.top;
    HDC screen = GetDC(nullptr);
    HDC memory = screen ? CreateCompatibleDC(screen) : nullptr;
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HBITMAP bitmap = memory && width > 0 && height > 0
        ? CreateDIBSection(screen, &info, DIB_RGB_COLORS, &bits, nullptr, 0)
        : nullptr;
    HGDIOBJ previous_bitmap = bitmap && memory ? SelectObject(memory, bitmap) : nullptr;
    bool rendered = false;
    ID2D1DCRenderTarget* target = nullptr;
    if (bits && previous_bitmap && previous_bitmap != HGDI_ERROR) {
        std::fill_n(static_cast<std::uint8_t*>(bits),
                    static_cast<std::size_t>(width) * height * 4,
                    std::uint8_t{0});
    }
    if (auto* factory = d2d_factory(); factory && bits && previous_bitmap
        && previous_bitmap != HGDI_ERROR) {
        D2D1_RENDER_TARGET_PROPERTIES properties{};
        properties.type = D2D1_RENDER_TARGET_TYPE_DEFAULT;
        properties.pixelFormat.format = DXGI_FORMAT_B8G8R8A8_UNORM;
        properties.pixelFormat.alphaMode = D2D1_ALPHA_MODE_PREMULTIPLIED;
        properties.usage = D2D1_RENDER_TARGET_USAGE_NONE;
        properties.minLevel = D2D1_FEATURE_LEVEL_DEFAULT;
        if (SUCCEEDED(factory->CreateDCRenderTarget(&properties, &target)) &&
            SUCCEEDED(target->BindDC(memory, &client))) {
            target->BeginDraw();
            target->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            target->Clear(D2D1::ColorF(0.0F, 0.0F, 0.0F, 0.0F));

            ID2D1SolidColorBrush* shadow = nullptr;
            ID2D1SolidColorBrush* background = nullptr;
            ID2D1SolidColorBrush* edge = nullptr;
            target->CreateSolidColorBrush(d2d_color(RGB(0, 0, 0), 46.0F / 255.0F),
                                          &shadow);
            target->CreateSolidColorBrush(d2d_color(RGB(28, 30, 36), 242.0F / 255.0F),
                                          &background);
            target->CreateSolidColorBrush(d2d_color(RGB(255, 255, 255), 26.0F / 255.0F),
                                          &edge);
            const float radius = static_cast<float>(height) * 0.5F;
            const auto pill = D2D1::RoundedRect(
                D2D1::RectF(0.5F, 0.5F, static_cast<float>(width) - 0.5F,
                            static_cast<float>(height) - 0.5F),
                radius, radius);
            const auto pill_shadow = D2D1::RoundedRect(
                D2D1::RectF(-0.1F, 0.7F, static_cast<float>(width) + 0.1F,
                            static_cast<float>(height) + 0.9F),
                radius, radius);
            if (shadow) target->FillRoundedRectangle(pill_shadow, shadow);
            if (background) target->FillRoundedRectangle(pill, background);
            if (edge) target->DrawRoundedRectangle(pill, edge, 1.0F);
            if (shadow) shadow->Release();
            if (background) background->Release();
            if (edge) edge->Release();

            ID2D1SolidColorBrush* dots = nullptr;
            target->CreateSolidColorBrush(d2d_color(RGB(255, 255, 255), 80.0F / 255.0F),
                                          &dots);
            if (dots) {
                const float scale = static_cast<float>(GetDpiForWindow(window_)) / 96.0F;
                const float cx = static_cast<float>(dpi_value(window_, logical_bar_padding))
                               + static_cast<float>(dpi_value(window_, logical_grip_width)) * 0.5F;
                const float cy = static_cast<float>(height) * 0.5F;
                for (const float dy : {-3.4F, 0.0F, 3.4F}) {
                    for (const float dx : {-1.7F, 1.7F}) {
                        target->FillEllipse(
                            D2D1::Ellipse(D2D1::Point2F(cx + dx * scale, cy + dy * scale),
                                         0.8F * scale, 0.8F * scale),
                            dots);
                    }
                }
                dots->Release();
            }

            for (int index = 0; index < static_cast<int>(command_ids.size()); ++index) {
                const RECT bounds = control_bar_button_rect(index);
                const bool enabled = index == 0
                    ? (editable_result_ && !loading_ && !refreshing_ &&
                       !pending_source_ && !editing_)
                    : true;
                const bool hovered = enabled && control_bar_hover_ == index;
                const bool pressed = enabled && control_bar_pressed_ == index;
                if (hovered || pressed) {
                    ID2D1SolidColorBrush* hover = nullptr;
                    if (index == 3) {
                        target->CreateSolidColorBrush(
                            d2d_color(RGB(232, 82, 72), hovered ? 1.0F : 205.0F / 255.0F),
                            &hover);
                    } else {
                        target->CreateSolidColorBrush(
                            d2d_color(RGB(255, 255, 255),
                                      pressed ? 74.0F / 255.0F : 46.0F / 255.0F),
                            &hover);
                    }
                    if (hover) {
                        const auto button = D2D1::RoundedRect(
                            D2D1::RectF(static_cast<float>(bounds.left) + 0.5F,
                                        static_cast<float>(bounds.top) + 0.5F,
                                        static_cast<float>(bounds.right) - 0.5F,
                                        static_cast<float>(bounds.bottom) - 0.5F),
                            static_cast<float>(dpi_value(window_, 5)),
                            static_cast<float>(dpi_value(window_, 5)));
                        target->FillRoundedRectangle(button, hover);
                        hover->Release();
                    }
                }
                const float inset = static_cast<float>(dpi_value(window_, 3));
                draw_control_icon(
                    target, index,
                    D2D1::RectF(static_cast<float>(bounds.left) + inset,
                                static_cast<float>(bounds.top) + inset,
                                static_cast<float>(bounds.right) - inset,
                                static_cast<float>(bounds.bottom) - inset),
                    RGB(255, 255, 255),
                    hovered ? 245.0F / 255.0F
                            : enabled ? 200.0F / 255.0F : 82.0F / 255.0F);
            }
            rendered = SUCCEEDED(target->EndDraw());
        }
    }
    if (target) target->Release();

    if (rendered && screen && memory) {
        RECT window_rect{};
        GetWindowRect(control_bar_, &window_rect);
        POINT destination{window_rect.left, window_rect.top};
        POINT source{0, 0};
        SIZE size{width, height};
        BLENDFUNCTION blend{AC_SRC_OVER, 0, 255, AC_SRC_ALPHA};
        if (!UpdateLayeredWindow(control_bar_, screen, &destination, &size,
                                 memory, &source, 0, &blend, ULW_ALPHA)) {
            OutputDebugStringW(L"ScreenTranslate: could not update result control bar.\n");
        }
    }
    if (memory && previous_bitmap && previous_bitmap != HGDI_ERROR) {
        SelectObject(memory, previous_bitmap);
    }
    if (bitmap) DeleteObject(bitmap);
    if (memory) DeleteDC(memory);
    if (screen) ReleaseDC(nullptr, screen);
    EndPaint(control_bar_, &state);
}

LRESULT CALLBACK ResultWindow::control_bar_proc(HWND window, UINT message,
                                                 WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<ResultWindow*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
        self = static_cast<ResultWindow*>(create->lpCreateParams);
        self->control_bar_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    return self ? self->handle_control_bar_message(message, wparam, lparam)
                : DefWindowProcW(window, message, wparam, lparam);
}

LRESULT ResultWindow::handle_control_bar_message(UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: paint_control_bar(); return 0;
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    case WM_SETCURSOR: {
        POINT cursor{};
        GetCursorPos(&cursor);
        ScreenToClient(control_bar_, &cursor);
        if (control_bar_dragging_ || control_bar_button_at(cursor) < 0) {
            SetCursor(LoadCursorW(nullptr, IDC_SIZEALL));
            return TRUE;
        }
        break;
    }
    case WM_MOUSEMOVE: {
        if (control_bar_dragging_) {
            POINT cursor{};
            if (GetCursorPos(&cursor)) {
                move_by(cursor.x - control_bar_drag_cursor_.x,
                        cursor.y - control_bar_drag_cursor_.y);
                control_bar_drag_cursor_ = cursor;
            }
            return 0;
        }
        const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        const int hovered = control_bar_button_at(point);
        if (hovered != control_bar_hover_) {
            control_bar_hover_ = hovered;
            InvalidateRect(control_bar_, nullptr, FALSE);
        }
        TRACKMOUSEEVENT track{sizeof(track), TME_LEAVE, control_bar_, 0};
        TrackMouseEvent(&track);
        return 0;
    }
    case WM_MOUSELEAVE:
        if (!control_bar_dragging_ && control_bar_hover_ != -1) {
            control_bar_hover_ = -1;
            InvalidateRect(control_bar_, nullptr, FALSE);
        }
        return 0;
    case WM_LBUTTONDOWN: {
        const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        control_bar_pressed_ = control_bar_button_at(point);
        SetCapture(control_bar_);
        if (control_bar_pressed_ < 0) {
            const bool can_drag = !recapturing_ && !resizing_;
            control_bar_dragging_ = can_drag &&
                GetCursorPos(&control_bar_drag_cursor_) != FALSE;
            if (control_bar_dragging_) SetCursor(LoadCursorW(nullptr, IDC_SIZEALL));
        }
        InvalidateRect(control_bar_, nullptr, FALSE);
        return 0;
    }
    case WM_LBUTTONUP: {
        const bool dragged = control_bar_dragging_;
        const int pressed = control_bar_pressed_;
        control_bar_dragging_ = false;
        control_bar_pressed_ = -1;
        if (GetCapture() == control_bar_) ReleaseCapture();
        const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        const int released = control_bar_button_at(point);
        InvalidateRect(control_bar_, nullptr, FALSE);
        if (!dragged && pressed >= 0 && pressed == released) {
            const bool enabled = pressed == 0
                ? (editable_result_ && !loading_ && !refreshing_ &&
                   !pending_source_ && !editing_)
                : pressed != 1 || (!loading_ && !refreshing_);
            const bool actionable = enabled &&
                (pressed != 1 || (!loading_ && !refreshing_));
            if (actionable && window_) {
                PostMessageW(window_, WM_COMMAND,
                             MAKEWPARAM(command_ids[static_cast<std::size_t>(pressed)], 0), 0);
            }
        }
        return 0;
    }
    case WM_CAPTURECHANGED:
        control_bar_dragging_ = false;
        control_bar_pressed_ = -1;
        InvalidateRect(control_bar_, nullptr, FALSE);
        return 0;
    case WM_DPICHANGED:
        update_control_bar_region();
        sync_control_bar();
        return 0;
    case WM_NCDESTROY: {
        const HWND old = control_bar_;
        SetWindowLongPtrW(old, GWLP_USERDATA, 0);
        control_bar_ = nullptr;
        return DefWindowProcW(old, message, wparam, lparam);
    }
    default: break;
    }
    return DefWindowProcW(control_bar_, message, wparam, lparam);
}

void ResultWindow::show_loading(const RECT& physical_rect,
                                std::shared_ptr<const PixelBuffer> original,
                                ResultAppearance appearance) {
    create_window();
    if (refreshing_) {
        KillTimer(window_, close_timer);
        KillTimer(window_, leave_timer);
        KillTimer(window_, recapture_restore_timer);
        SetTimer(window_, progress_timer, 16, nullptr);
        pending_source_ = PendingSource{
            physical_rect, std::move(original), std::move(appearance),
            {physical_rect.left, physical_rect.top}};
        recapturing_ = false;
        editing_ = false;
        error_.clear();
        handles_alpha_ = 0.0;
        set_capture_exclusion(false);
        InvalidateRect(window_, nullptr, FALSE);
        UpdateWindow(window_);
        SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
        sync_control_bar();
        if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
        return;
    }

    KillTimer(window_, recapture_restore_timer);
    recapturing_ = false;
    pending_source_.reset();
    refresh_surface_ = {};
    const bool same_source = original_ && original && original_.get() == original.get();
    KillTimer(window_, close_timer);
    KillTimer(window_, leave_timer);
    original_ = std::move(original);
    translated_surface_ = {};
    appearance_ = std::move(appearance);
    result_ = {};
    plain_text_.clear();
    error_.clear();
    captured_rect_ = physical_rect;
    image_screen_origin_ = {physical_rect.left, physical_rect.top};
    if (!home_initialized_) {
        home_rect_ = physical_rect;
        home_initialized_ = true;
        const int selected_width = std::max(
            1, static_cast<int>(physical_rect.right - physical_rect.left));
        const int selected_height = std::max(
            1, static_cast<int>(physical_rect.bottom - physical_rect.top));
        minimum_width_ = std::min(selected_width, dpi_value(window_, 72));
        minimum_height_ = std::min(selected_height, dpi_value(window_, 32));
    }
    if (!same_source) editable_result_ = false;
    loading_ = true;
    progress_phase_ = 0.0;
    SetTimer(window_, progress_timer, 16, nullptr);
    successful_ = false;
    editing_ = false;
    peek_ = false;
    closing_ = false;
    handles_alpha_ = 0.0;
    const int width = std::max(1, static_cast<int>(physical_rect.right - physical_rect.left));
    const int height = std::max(1, static_cast<int>(physical_rect.bottom - physical_rect.top));
    SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
    SetWindowPos(window_, HWND_TOPMOST, physical_rect.left, physical_rect.top,
                 width, height, SWP_SHOWWINDOW);
    ShowWindow(window_, SW_SHOW);
    SetForegroundWindow(window_);
    SetFocus(window_);
    update_window_region();
    sync_control_bar();
    if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
    InvalidateRect(window_, nullptr, FALSE);
    UpdateWindow(window_);
    set_capture_exclusion(false);
}

void ResultWindow::show_retry_loading(ResultAppearance appearance) {
    if (!window_) return;
    KillTimer(window_, close_timer);
    KillTimer(window_, leave_timer);
    KillTimer(window_, recapture_restore_timer);
    recapturing_ = false;
    if (pending_source_) {
        pending_source_->appearance = std::move(appearance);
        refreshing_ = true;
        loading_ = false;
        editing_ = false;
        error_.clear();
        handles_alpha_ = 0.0;
        SetTimer(window_, progress_timer, 16, nullptr);
        InvalidateRect(window_, nullptr, FALSE);
        UpdateWindow(window_);
        sync_control_bar();
        if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
        return;
    }

    appearance_ = std::move(appearance);
    loading_ = true;
    SetTimer(window_, progress_timer, 16, nullptr);
    successful_ = false;
    editing_ = false;
    peek_ = false;
    error_.clear();
    handles_alpha_ = 0.0;
    SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
    sync_control_bar();
    if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
    InvalidateRect(window_, nullptr, FALSE);
    UpdateWindow(window_);
}

void ResultWindow::set_result(PipelineResult result) {
    if (!window_) return;
    KillTimer(window_, progress_timer);
    KillTimer(window_, recapture_restore_timer);
    const bool commit_refresh = refreshing_ && pending_source_.has_value();
    if (commit_refresh) {
        original_ = std::move(pending_source_->original);
        appearance_ = std::move(pending_source_->appearance);
        captured_rect_ = pending_source_->rect;
        image_screen_origin_ = pending_source_->image_origin;
        recapturing_ = false;
        if (!resizing_) set_capture_exclusion(false);
    }
    result_ = std::move(result);
    build_repaired_background();
    plain_text_ = result_.plain_text;
    loading_ = false;
    successful_ = true;
    editable_result_ = !result_.blocks.empty();
    error_.clear();
    refreshing_ = false;
    pending_source_.reset();
    refresh_surface_ = {};
    RedrawWindow(window_, nullptr, nullptr,
                 RDW_INVALIDATE | RDW_UPDATENOW);
    SetLayeredWindowAttributes(
        window_, resizing_ ? resize_transparent_key : 0, 255,
        resizing_ ? (LWA_ALPHA | LWA_COLORKEY) : LWA_ALPHA);
    sync_control_bar();
    if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
    arm_close_timer();
}

void ResultWindow::set_error(std::wstring message) {
    if (!window_) return;
    KillTimer(window_, progress_timer);
    KillTimer(window_, close_timer);
    KillTimer(window_, leave_timer);
    KillTimer(window_, recapture_restore_timer);
    if (refreshing_) {
        recapturing_ = false;
        refreshing_ = false;
        set_capture_exclusion(false);
        SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
        loading_ = false;
        editing_ = false;
        handles_alpha_ = 0.0;
        error_.clear();
        pending_source_.reset();
        refresh_surface_ = {};
        const std::wstring diagnostic = L"ScreenTranslate refresh failed: " + message + L"\n";
        OutputDebugStringW(diagnostic.c_str());
        InvalidateRect(window_, nullptr, FALSE);
        UpdateWindow(window_);
        sync_control_bar();
        if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
        arm_close_timer();
        return;
    }
    if (recapturing_) {
        recapturing_ = false;
        set_capture_exclusion(false);
        SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
        sync_control_bar();
    }
    translated_surface_ = {};
    loading_ = false;
    successful_ = false;
    error_ = std::move(message);
    InvalidateRect(window_, nullptr, FALSE);
    if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
}

void ResultWindow::close() {
    if (!window_ || closing_) return;
    closing_ = true;
    KillTimer(window_, close_timer);
    KillTimer(window_, leave_timer);
    KillTimer(window_, progress_timer);
    content_dragging_ = false;
    destroy_control_bar();
    const HWND old = window_;
    DestroyWindow(old);
    window_ = nullptr;
    release_paint_buffer();
    original_.reset();
    pending_source_.reset();
    refresh_surface_ = {};
    translated_surface_ = {};
    result_ = {};
    plain_text_.clear();
    error_.clear();
    editable_result_ = false;
    home_initialized_ = false;
    resizing_ = false;
    recapturing_ = false;
    refreshing_ = false;
    refresh_was_active_on_resize_ = false;
    recapture_exclusion_active_ = false;
    control_bar_capture_excluded_ = false;
    content_dragging_ = false;
    freeze_content_only_ = false;
    handles_alpha_ = 0.0;
    progress_phase_ = 0.0;
    const auto callback = closed_callback_;
    closing_ = false;
    if (callback) callback();
}

void ResultWindow::minimize() {
    if (window_) {
        KillTimer(window_, close_timer);
        KillTimer(window_, leave_timer);
        if (!recapturing_) KillTimer(window_, recapture_restore_timer);
        ShowWindow(window_, SW_HIDE);
        if (control_bar_) ShowWindow(control_bar_, SW_HIDE);
        SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
        content_dragging_ = false;
        if (GetCapture() == window_) ReleaseCapture();
    }
}

void ResultWindow::restore() {
    if (!window_) return;
    ShowWindow(window_, SW_SHOW);
    SetWindowPos(window_, HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    SetForegroundWindow(window_);
    SetFocus(window_);
    sync_control_bar();
    arm_close_timer();
}

void ResultWindow::set_editing(bool value) {
    editing_ = value;
    if (!window_) return;
    if (value) {
        KillTimer(window_, close_timer);
        KillTimer(window_, leave_timer);
    } else {
        arm_close_timer();
    }
    if (control_bar_) InvalidateRect(control_bar_, nullptr, FALSE);
}

bool ResultWindow::visible() const noexcept {
    return window_ && IsWindowVisible(window_);
}

void ResultWindow::build_repaired_background() {
    translated_surface_ = {};
    if (!original_ || original_->empty()) return;
    auto prepared = prepare_result_render(
        *original_, result_.blocks, appearance_.font_family,
        appearance_.minimum_font_pixels);
    translated_surface_ = std::move(prepared.translated);
}

void ResultWindow::set_peek(bool value) {
    if (refreshing_ || !refresh_surface_.empty()) return;
    if (peek_ == value) return;
    peek_ = value;
    RedrawWindow(window_, nullptr, nullptr, RDW_INVALIDATE | RDW_UPDATENOW);
}

void ResultWindow::copy_all() {
    if (copy_callback_) copy_callback_(plain_text_);
}

void ResultWindow::move_by(int dx, int dy) {
    if (!window_ || recapturing_ || resizing_ || (dx == 0 && dy == 0)) {
        return;
    }
    RECT rect{};
    GetWindowRect(window_, &rect);
    image_screen_origin_.x += dx;
    image_screen_origin_.y += dy;
    if (pending_source_) {
        pending_source_->image_origin.x += dx;
        pending_source_->image_origin.y += dy;
    }
    SetWindowPos(window_, HWND_TOPMOST, rect.left + dx, rect.top + dy, 0, 0,
                 SWP_NOSIZE | SWP_NOACTIVATE);
    InvalidateRect(window_, nullptr, FALSE);
}

void ResultWindow::reset_geometry() {
    if (!window_ || !home_initialized_ || resizing_) return;
    RECT current{};
    GetWindowRect(window_, &current);
    if (equal_rect(current, home_rect_) && equal_rect(captured_rect_, home_rect_)) return;
    const bool needs_recapture = !equal_rect(captured_rect_, home_rect_);
    if (!needs_recapture) {
        set_capture_exclusion(false);
        refresh_surface_ = {};
        refreshing_ = false;
        recapturing_ = false;
        pending_source_.reset();
        image_screen_origin_ = {home_rect_.left, home_rect_.top};
        SetWindowPos(window_, HWND_TOPMOST, home_rect_.left, home_rect_.top,
                     home_rect_.right - home_rect_.left,
                     home_rect_.bottom - home_rect_.top, SWP_SHOWWINDOW);
        update_window_region();
        sync_control_bar();
        RedrawWindow(window_, nullptr, nullptr, RDW_INVALIDATE | RDW_UPDATENOW);
        arm_close_timer();
        return;
    }
    freeze_current_surface();
    refreshing_ = true;
    pending_source_.reset();
    if (control_bar_) ShowWindow(control_bar_, SW_HIDE);
    image_screen_origin_ = {home_rect_.left, home_rect_.top};
    SetWindowPos(window_, HWND_TOPMOST, home_rect_.left, home_rect_.top,
                 home_rect_.right - home_rect_.left,
                 home_rect_.bottom - home_rect_.top, SWP_SHOWWINDOW);
    update_window_region();
    if (recapture_callback_) {
        recapturing_ = true;
        handles_alpha_ = 0.0;
        set_capture_exclusion(false);
        SetLayeredWindowAttributes(window_, 0, 0, LWA_ALPHA);
        DwmFlush();
        SetTimer(window_, recapture_restore_timer, 1000, nullptr);
        recapture_callback_(home_rect_);
    }
}

void ResultWindow::freeze_current_surface() noexcept {
    if (!window_ || !IsWindowVisible(window_) || !refresh_surface_.empty()) {
        return;
    }
    const double previous_handles_alpha = handles_alpha_;
    handles_alpha_ = 0.0;
    freeze_content_only_ = true;
    if (control_bar_) ShowWindow(control_bar_, SW_HIDE);
    RedrawWindow(window_, nullptr, nullptr,
                 RDW_INVALIDATE | RDW_UPDATENOW);
    DwmFlush();
    RECT bounds{};
    if (!GetWindowRect(window_, &bounds) || bounds.right <= bounds.left
        || bounds.bottom <= bounds.top) {
        freeze_content_only_ = false;
        handles_alpha_ = previous_handles_alpha;
        return;
    }
    try {
        refresh_surface_ = capture_rect(bounds);
    } catch (...) {
        if (!translated_surface_.empty()) {
            refresh_surface_ = translated_surface_;
        } else if (original_ && !original_->empty()) {
            refresh_surface_ = *original_;
        }
        OutputDebugStringW(L"ScreenTranslate: could not freeze the current result surface.\n");
    }
    freeze_content_only_ = false;
    handles_alpha_ = previous_handles_alpha;
}

void ResultWindow::aim_handles(double target) {
    const double value = recapturing_ ? 0.0 : std::clamp(target, 0.0, 1.0);
    if (std::abs(value - handles_alpha_) <= 0.01) return;
    handles_alpha_ = value;
    if (window_ && IsWindowVisible(window_)) {
        InvalidateRect(window_, nullptr, FALSE);
    }
}

bool ResultWindow::set_capture_exclusion(bool exclude) noexcept {
    if (!window_) return false;
    if (!exclude) {
        if (!recapture_exclusion_active_ && !control_bar_capture_excluded_) return true;
        SetWindowDisplayAffinity(window_, WDA_NONE);
        if (control_bar_) SetWindowDisplayAffinity(control_bar_, WDA_NONE);
        recapture_exclusion_active_ = false;
        control_bar_capture_excluded_ = false;
        return true;
    }

    const bool main_excluded =
        SetWindowDisplayAffinity(window_, WDA_EXCLUDEFROMCAPTURE) != FALSE;
    const bool bar_excluded = !control_bar_ ||
        SetWindowDisplayAffinity(control_bar_, WDA_EXCLUDEFROMCAPTURE) != FALSE;
    recapture_exclusion_active_ = main_excluded;
    control_bar_capture_excluded_ = bar_excluded;
    if (!main_excluded) {
        if (control_bar_) SetWindowDisplayAffinity(control_bar_, WDA_NONE);
        control_bar_capture_excluded_ = false;
        return false;
    }
    // The affinity must reach the compositor before GDI captures the desktop.
    DwmFlush();
    return true;
}

void ResultWindow::arm_close_timer() {
    KillTimer(window_, close_timer);
    KillTimer(window_, leave_timer);
    if (!successful_ || editing_ || resizing_ || content_dragging_ ||
        !IsWindowVisible(window_)) {
        return;
    }
    if (appearance_.close_mode == L"timeout" && appearance_.timeout_ms > 0) {
        SetTimer(window_, close_timer, appearance_.timeout_ms, nullptr);
    } else if (appearance_.close_mode == L"leave") {
        SetTimer(window_, leave_timer, 150, nullptr);
    }
}

void ResultWindow::paint() {
    PAINTSTRUCT state{};
    HDC target = BeginPaint(window_, &state);
    if (!target) return;
    RECT client{};
    GetClientRect(window_, &client);
    const int client_width = static_cast<int>(client.right - client.left);
    const int client_height = static_cast<int>(client.bottom - client.top);
    HDC buffer = ensure_paint_buffer(target, client_width, client_height);
    HDC dc = buffer ? buffer : target;
    HBRUSH empty_background = CreateSolidBrush(
        resizing_ ? resize_transparent_key : RGB(18, 20, 24));
    FillRect(dc, &client, empty_background);
    DeleteObject(empty_background);
    const auto draw_surface = [&](const PixelBuffer& surface,
                                  int destination_x, int destination_y,
                                  int source_x, int source_y,
                                  int width, int height) {
        if (surface.empty() || width <= 0 || height <= 0) return;
        BITMAPINFO info{};
        info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        info.bmiHeader.biWidth = surface.width;
        info.bmiHeader.biHeight = -surface.height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        SetStretchBltMode(dc, COLORONCOLOR);
        StretchDIBits(dc, destination_x, destination_y, width, height,
                      source_x, source_y, width, height,
                      surface.bgra.data(), &info, DIB_RGB_COLORS, SRCCOPY);
    };
    RECT window_rect{};
    GetWindowRect(window_, &window_rect);
    if (resizing_) {
        // The color-keyed interior reveals the compositor's live desktop.
    } else if (pending_source_ && pending_source_->original
               && !pending_source_->original->empty()) {
        draw_surface(*pending_source_->original,
                     pending_source_->image_origin.x - window_rect.left,
                     pending_source_->image_origin.y - window_rect.top,
                     0, 0, pending_source_->original->width,
                     pending_source_->original->height);
    } else if (!refresh_surface_.empty()) {
        draw_surface(refresh_surface_, 0, 0, 0, 0,
                     refresh_surface_.width, refresh_surface_.height);
    } else {
        const PixelBuffer* display = nullptr;
        if (!peek_ && !translated_surface_.empty()) {
            display = &translated_surface_;
        } else if (original_ && !original_->empty()) {
            display = original_.get();
        }
        if (display) {
        const int offset_x = image_screen_origin_.x - window_rect.left;
        const int offset_y = image_screen_origin_.y - window_rect.top;
            draw_surface(*display, offset_x, offset_y, 0, 0,
                         display->width, display->height);
        }
    }

    if (!resizing_ && (loading_ || refreshing_)) {
        alpha_fill(dc, client, RGB(0, 0, 0), 46);
        const int progress_width = std::min(
            client_width,
            std::max(dpi_value(window_, 28), client_width / 4));
        const int travel = std::max(1, client_width - progress_width);
        const double cycle = progress_phase_ * 2.0;
        const double position = cycle <= 1.0 ? cycle : 2.0 - cycle;
        const double eased = position * position * (3.0 - 2.0 * position);
        const int left = static_cast<int>(std::lround(eased * travel));
        RECT progress{left, 0, std::min(client_width, left + progress_width),
                      std::min(client_height, dpi_value(window_, 2))};
        HBRUSH accent = CreateSolidBrush(appearance_.accent);
        FillRect(dc, &progress, accent);
        DeleteObject(accent);
    } else if (!resizing_ && !error_.empty()) {
        alpha_fill(dc, client, RGB(24, 24, 28), 232);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, RGB(240, 240, 245));
        const int font_pixels = std::clamp(client_height / 6,
                                           dpi_value(window_, 11),
                                           dpi_value(window_, 14));
        HFONT font = CreateFontW(-font_pixels, 0, 0, 0, FW_NORMAL,
                                 FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                                 OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                                 CLEARTYPE_QUALITY, DEFAULT_PITCH,
                                 appearance_.font_family.c_str());
        const HGDIOBJ old_font = SelectObject(dc, font);
        const int horizontal = dpi_value(window_, 10);
        const int vertical = dpi_value(window_, 8);
        RECT text_rect{client.left + horizontal, client.top + vertical,
                       client.right - horizontal, client.bottom - vertical};
        DrawTextW(dc, error_.c_str(), -1, &text_rect,
                  DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(dc, old_font);
        DeleteObject(font);
    }

    const int width = client_width;
    const int height = client_height;
    if (width > 0 && height > 0 && !freeze_content_only_) {
        const int border = std::min(dpi_value(window_, 2), std::max(1, std::min(width, height) / 2));
        const int dark_border = std::min(dpi_value(window_, 1),
                                         std::max(1, std::min(width, height) / 2));
        const RECT frame_bounds{0, 0, width, height};
        alpha_frame(dc, frame_bounds, dark_border, RGB(0, 0, 0), 90);
        const RECT inner_dark{border, border, width - border, height - border};
        if (inner_dark.right > inner_dark.left && inner_dark.bottom > inner_dark.top) {
            alpha_frame(dc, inner_dark, dark_border, RGB(0, 0, 0), 90);
        }
        alpha_frame(dc, frame_bounds, border, appearance_.accent, 255);

        const int handle = dpi_value(window_, 8);
        if (handles_alpha_ > 0.04 && width >= dpi_value(window_, 56)
            && height >= dpi_value(window_, 34)) {
            const int xs[3]{0, (width - handle) / 2, width - handle};
            const int ys[3]{0, (height - handle) / 2, height - handle};
            constexpr std::array<std::pair<int, int>, 8> positions{{
                {0, 0}, {1, 0}, {2, 0}, {0, 1},
                {2, 1}, {0, 2}, {1, 2}, {2, 2},
            }};
            const int corner = dpi_value(window_, 2);
            for (const auto [column, row] : positions) {
                const RECT handle_bounds{xs[column], ys[row],
                                         xs[column] + handle, ys[row] + handle};
                draw_alpha_handle(dc, handle_bounds, appearance_.accent,
                                  handles_alpha_, corner);
            }
        }

        if (resizing_) {
            const auto badge = make_size_badge(window_, width, height);
            HFONT badge_font = CreateFontW(-badge.font_pixels, 0, 0, 0, FW_NORMAL,
                                            FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                                            OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                                            CLEARTYPE_QUALITY, DEFAULT_PITCH,
                                            L"Microsoft YaHei UI");
            const HGDIOBJ old_badge_font = SelectObject(dc, badge_font);
            const int badge_height = badge.bounds.bottom - badge.bounds.top;
            HRGN badge_region = CreateRoundRectRgn(
                badge.bounds.left, badge.bounds.top, badge.bounds.right,
                badge.bounds.bottom, badge_height, badge_height);
            if (badge_region) {
                alpha_fill_region(dc, badge_region, badge.bounds, RGB(18, 20, 24), 232);
                DeleteObject(badge_region);
            }
            const int red = std::min(255, static_cast<int>(GetRValue(appearance_.accent)) * 118 / 100);
            const int green = std::min(255, static_cast<int>(GetGValue(appearance_.accent)) * 118 / 100);
            const int blue = std::min(255, static_cast<int>(GetBValue(appearance_.accent)) * 118 / 100);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, RGB(red, green, blue));
            RECT badge_text = badge.bounds;
            DrawTextW(dc, badge.text.c_str(), -1, &badge_text,
                      DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            SelectObject(dc, old_badge_font);
            DeleteObject(badge_font);
        }
    }
    if (buffer) {
        BitBlt(target, 0, 0, client_width, client_height, buffer, 0, 0, SRCCOPY);
    }
    EndPaint(window_, &state);
}

LRESULT CALLBACK ResultWindow::window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<ResultWindow*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
        self = static_cast<ResultWindow*>(create->lpCreateParams);
        self->window_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    return self ? self->handle_message(message, wparam, lparam)
                : DefWindowProcW(window, message, wparam, lparam);
}


LRESULT ResultWindow::handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: paint(); return 0;
    case WM_MOVE:
        sync_control_bar();
        return 0;
    case WM_SIZE:
        update_window_region();
        sync_control_bar();
        if (resizing_) {
            RedrawWindow(window_, nullptr, nullptr, RDW_INVALIDATE | RDW_NOERASE);
        } else {
            InvalidateRect(window_, nullptr, FALSE);
        }
        return 0;
    case WM_MOUSEMOVE: {
        if (content_dragging_) {
            POINT cursor{};
            if (GetCursorPos(&cursor)) {
                move_by(cursor.x - content_drag_cursor_.x,
                        cursor.y - content_drag_cursor_.y);
                content_drag_cursor_ = cursor;
            }
        }
        aim_handles(1.0);
        TRACKMOUSEEVENT track{sizeof(track), TME_LEAVE, window_, 0};
        TrackMouseEvent(&track);
        return 0;
    }
    case WM_MOUSELEAVE:
        if (!resizing_) aim_handles(0.0);
        return 0;
    case WM_LBUTTONDOWN:
        SetFocus(window_);
        if (!recapturing_ && !resizing_ && GetCursorPos(&content_drag_cursor_)) {
            KillTimer(window_, close_timer);
            KillTimer(window_, leave_timer);
            content_dragging_ = true;
            SetCapture(window_);
        }
        return 0;
    case WM_LBUTTONUP:
        if (content_dragging_) {
            content_dragging_ = false;
            if (GetCapture() == window_) ReleaseCapture();
            arm_close_timer();
        }
        return 0;
    case WM_SETCURSOR:
        if (LOWORD(lparam) == HTCLIENT && !recapturing_) {
            SetCursor(LoadCursorW(nullptr, IDC_SIZEALL));
            return TRUE;
        }
        break;
    case WM_COMMAND:
        switch (LOWORD(wparam)) {
        case 4102:
            if (!loading_ && !refreshing_ && retry_callback_) retry_callback_();
            return 0;
        case 4103:
            if (editable_result_ && !loading_ && !refreshing_ &&
                !pending_source_ && !editing_ && edit_callback_) {
                edit_callback_();
            }
            return 0;
        case 4104: minimize(); return 0;
        case 4105: close(); return 0;
        default: break;
        }
        break;
    case WM_KEYDOWN:
        {
        const bool ctrl = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
        const int step = (GetKeyState(VK_SHIFT) & 0x8000) != 0 ? 10 : 1;
        if (wparam == VK_ESCAPE) { close(); return 0; }
        if (wparam == L'R' && !ctrl && !loading_ && !refreshing_ && retry_callback_) {
            retry_callback_();
            return 0;
        }
        if (wparam == L'E' && !ctrl && editable_result_ && !loading_ &&
            !refreshing_ && !pending_source_ && !editing_ && edit_callback_) {
            edit_callback_();
            return 0;
        }
        if (wparam == L'M' || wparam == VK_OEM_MINUS) { minimize(); return 0; }
        if (wparam == VK_SPACE) { set_peek(true); return 0; }
        if (ctrl && wparam == L'C') {
            copy_all();
            return 0;
        }
        if (ctrl && wparam == L'A') {
            copy_all();
            return 0;
        }
        if (wparam == VK_LEFT || wparam == VK_RIGHT ||
            wparam == VK_UP || wparam == VK_DOWN) {
            int dx = 0, dy = 0;
            if (wparam == VK_LEFT) dx = -step;
            if (wparam == VK_RIGHT) dx = step;
            if (wparam == VK_UP) dy = -step;
            if (wparam == VK_DOWN) dy = step;
            move_by(dx, dy);
            return 0;
        }
        if (wparam == VK_HOME) {
            reset_geometry();
            return 0;
        }
        }
        break;
    case WM_KEYUP:
        if (wparam == VK_SPACE) { set_peek(false); return 0; }
        break;
    case WM_RBUTTONDOWN: set_peek(true); SetCapture(window_); return 0;
    case WM_RBUTTONUP: ReleaseCapture(); set_peek(false); return 0;
    case WM_CONTEXTMENU: return 0;
    case WM_CANCELMODE:
        content_dragging_ = false;
        if (GetCapture() == window_) ReleaseCapture();
        if (peek_) set_peek(false);
        arm_close_timer();
        return 0;
    case WM_CAPTURECHANGED:
        if (content_dragging_) {
            content_dragging_ = false;
            arm_close_timer();
        }
        return 0;
    case WM_GETMINMAXINFO: {
        auto* info = reinterpret_cast<MINMAXINFO*>(lparam);
        info->ptMinTrackSize.x = std::max(1, minimum_width_);
        info->ptMinTrackSize.y = std::max(1, minimum_height_);
        return 0;
    }
    case WM_NCHITTEST: {
        const LRESULT base = DefWindowProcW(window_, message, wparam, lparam);
        if (base != HTCLIENT) return base;
        if (recapturing_) return HTCLIENT;
        RECT window_rect{};
        GetWindowRect(window_, &window_rect);
        const int x = GET_X_LPARAM(lparam) - window_rect.left;
        const int y = GET_Y_LPARAM(lparam) - window_rect.top;
        const int width = window_rect.right - window_rect.left;
        const int height = window_rect.bottom - window_rect.top;
        const int edge = dpi_value(window_, logical_border_hit);
        const bool left = x < edge, right = x >= width - edge;
        const bool top = y < edge, bottom = y >= height - edge;
        if (top && left) return HTTOPLEFT;
        if (top && right) return HTTOPRIGHT;
        if (bottom && left) return HTBOTTOMLEFT;
        if (bottom && right) return HTBOTTOMRIGHT;
        if (left) return HTLEFT;
        if (right) return HTRIGHT;
        if (top) return HTTOP;
        if (bottom) return HTBOTTOM;
        return HTCLIENT;
    }
    case WM_ENTERSIZEMOVE: {
        GetWindowRect(window_, &resize_start_rect_);
        refresh_was_active_on_resize_ =
            refreshing_ || pending_source_.has_value();
        resizing_ = true;
        content_dragging_ = false;
        handles_alpha_ = 1.0;
        KillTimer(window_, close_timer);
        KillTimer(window_, leave_timer);
        set_capture_exclusion(false);
        SetLayeredWindowAttributes(window_, resize_transparent_key, 255,
                                   LWA_ALPHA | LWA_COLORKEY);
        sync_control_bar();
        RedrawWindow(window_, nullptr, nullptr, RDW_INVALIDATE | RDW_UPDATENOW);
        return 0;
    }
    case WM_SIZING: {
        auto* rect = reinterpret_cast<RECT*>(lparam);
        const bool moves_left = wparam == WMSZ_LEFT || wparam == WMSZ_TOPLEFT
                             || wparam == WMSZ_BOTTOMLEFT;
        const bool moves_right = wparam == WMSZ_RIGHT || wparam == WMSZ_TOPRIGHT
                              || wparam == WMSZ_BOTTOMRIGHT;
        const bool moves_top = wparam == WMSZ_TOP || wparam == WMSZ_TOPLEFT
                            || wparam == WMSZ_TOPRIGHT;
        const bool moves_bottom = wparam == WMSZ_BOTTOM || wparam == WMSZ_BOTTOMLEFT
                               || wparam == WMSZ_BOTTOMRIGHT;
        const bool corner = wparam == WMSZ_TOPLEFT || wparam == WMSZ_TOPRIGHT
                         || wparam == WMSZ_BOTTOMLEFT || wparam == WMSZ_BOTTOMRIGHT;
        if (corner && (GetKeyState(VK_SHIFT) & 0x8000) != 0) {
            const int base_width = std::max(
                1, static_cast<int>(resize_start_rect_.right - resize_start_rect_.left));
            const int base_height = std::max(
                1, static_cast<int>(resize_start_rect_.bottom - resize_start_rect_.top));
            int width = std::max(minimum_width_, static_cast<int>(rect->right - rect->left));
            int height = std::max(minimum_height_, static_cast<int>(rect->bottom - rect->top));
            const double denominator = static_cast<double>(base_width) * base_width
                                     + static_cast<double>(base_height) * base_height;
            double scale = 1.0
                + (static_cast<double>(width - base_width) * base_width
                   + static_cast<double>(height - base_height) * base_height)
                    / denominator;
            scale = std::max({scale,
                              static_cast<double>(minimum_width_) / base_width,
                              static_cast<double>(minimum_height_) / base_height});
            width = std::max(minimum_width_,
                             static_cast<int>(std::lround(base_width * scale)));
            height = std::max(minimum_height_,
                              static_cast<int>(std::lround(base_height * scale)));
            if (wparam == WMSZ_TOPLEFT || wparam == WMSZ_BOTTOMLEFT) rect->left = rect->right - width;
            else rect->right = rect->left + width;
            if (wparam == WMSZ_TOPLEFT || wparam == WMSZ_TOPRIGHT) rect->top = rect->bottom - height;
            else rect->bottom = rect->top + height;
        }
        MONITORINFO monitor{};
        monitor.cbSize = sizeof(monitor);
        if (GetMonitorInfoW(MonitorFromRect(&resize_start_rect_, MONITOR_DEFAULTTONEAREST),
                            &monitor)) {
            if (moves_left) rect->left = std::max(rect->left, monitor.rcMonitor.left);
            if (moves_right) rect->right = std::min(rect->right, monitor.rcMonitor.right);
            if (moves_top) rect->top = std::max(rect->top, monitor.rcMonitor.top);
            if (moves_bottom) rect->bottom = std::min(rect->bottom, monitor.rcMonitor.bottom);
        }
        return TRUE;
    }
    case WM_EXITSIZEMOVE: {
        RECT rect{}; GetWindowRect(window_, &rect);
        resizing_ = false;
        POINT cursor{};
        GetCursorPos(&cursor);
        aim_handles(PtInRect(&rect, cursor) ? 1.0 : 0.0);
        const bool region_changed = !equal_rect(rect, resize_start_rect_)
                                 || !equal_size(rect, captured_rect_);
        if (recapture_callback_ && region_changed) {
            refreshing_ = true;
            recapturing_ = true;
            pending_source_.reset();
            handles_alpha_ = 0.0;
            set_capture_exclusion(false);
            if (control_bar_) ShowWindow(control_bar_, SW_HIDE);
            SetLayeredWindowAttributes(window_, 0, 0, LWA_ALPHA);
            DwmFlush();
            SetTimer(window_, recapture_restore_timer, 1000, nullptr);
            recapture_callback_(rect);
        } else if (refresh_was_active_on_resize_
                   && (refreshing_ || pending_source_.has_value())) {
            set_capture_exclusion(false);
            refreshing_ = true;
            SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
            sync_control_bar();
            InvalidateRect(window_, nullptr, FALSE);
        } else {
            set_capture_exclusion(false);
            refreshing_ = false;
            refresh_surface_ = {};
            SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
            update_window_region();
            sync_control_bar();
            RedrawWindow(window_, nullptr, nullptr, RDW_INVALIDATE | RDW_UPDATENOW);
            arm_close_timer();
        }
        refresh_was_active_on_resize_ = false;
        return 0;
    }
    case WM_LBUTTONDBLCLK:
        reset_geometry();
        return 0;
    case WM_TIMER:
        if (wparam == progress_timer) {
            progress_phase_ = std::fmod(progress_phase_ + 0.022, 1.0);
            if (window_ && IsWindowVisible(window_)) {
                InvalidateRect(window_, nullptr, FALSE);
            }
            return 0;
        }
        if (wparam == recapture_restore_timer) {
            KillTimer(window_, recapture_restore_timer);
            if (recapturing_) {
                recapturing_ = false;
                set_capture_exclusion(false);
                SetLayeredWindowAttributes(window_, 0, 255, LWA_ALPHA);
                if (IsWindowVisible(window_)) sync_control_bar();
                InvalidateRect(window_, nullptr, FALSE);
            }
            return 0;
        }
        if (wparam == close_timer) { close(); return 0; }
        if (wparam == leave_timer) {
            if (resizing_) return 0;
            POINT cursor{};
            RECT result_rect{};
            RECT bar_rect{};
            const bool have_cursor = GetCursorPos(&cursor) != FALSE;
            const bool over_result = have_cursor && GetWindowRect(window_, &result_rect)
                                  && PtInRect(&result_rect, cursor);
            const bool over_bar = have_cursor && control_bar_ && IsWindowVisible(control_bar_)
                               && GetWindowRect(control_bar_, &bar_rect)
                               && PtInRect(&bar_rect, cursor);
            if (have_cursor && !over_result && !over_bar) {
                close();
            }
            return 0;
        }
        break;
    case WM_DPICHANGED:
        update_control_bar_region();
        sync_control_bar();
        return 0;
    case WM_CLOSE: close(); return 0;
    case WM_NCDESTROY: {
        const HWND old = window_;
        KillTimer(old, close_timer);
        KillTimer(old, leave_timer);
        KillTimer(old, recapture_restore_timer);
        if (control_bar_) destroy_control_bar();
        SetWindowLongPtrW(old, GWLP_USERDATA, 0);
        if (!closing_) window_ = nullptr;
        return DefWindowProcW(old, message, wparam, lparam);
    }
    default: break;
    }
    return DefWindowProcW(window_, message, wparam, lparam);
}

}  // namespace screentrans
