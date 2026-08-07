#include "app.hpp"

#include "block_editor.hpp"
#include "hotkey.hpp"
#include "resource.h"
#include "util.hpp"

#include <commctrl.h>
#include <dwmapi.h>
#include <shellapi.h>
#include <shellscalingapi.h>
#include <uxtheme.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <cwchar>
#include <filesystem>
#include <vector>

namespace screentrans {

namespace {

constexpr UINT tray_message = WM_APP + 2;
constexpr UINT initialize_message = WM_APP + 3;
constexpr UINT_PTR capture_timer = 1;
constexpr UINT_PTR recapture_timer = 2;
constexpr UINT_PTR update_timer = 3;
constexpr UINT_PTR text_translate_timer = 4;
constexpr UINT silent_update_completed_message = WM_APP + 41;
constexpr int hotkey_capture = 1;
constexpr int hotkey_toggle = 2;
constexpr int hotkey_text_translate = 3;
constexpr UINT command_capture = 3001;
constexpr UINT command_restore = 3002;
constexpr UINT command_settings = 3003;
constexpr UINT command_updates = 3004;
constexpr UINT command_restart = 3005;
constexpr UINT command_exit = 3006;
constexpr UINT command_text_translate = 3007;

constexpr COLORREF tray_color_background = RGB(32, 32, 32);
constexpr COLORREF tray_color_border = RGB(58, 58, 58);
constexpr COLORREF tray_color_separator = RGB(61, 61, 61);
constexpr COLORREF tray_color_hover = RGB(48, 48, 48);
constexpr COLORREF tray_color_text = RGB(243, 243, 243);
constexpr COLORREF tray_color_text_dim = RGB(178, 178, 178);
constexpr COLORREF tray_color_text_faint = RGB(112, 112, 112);
constexpr int tray_menu_min_width = 240;
constexpr int tray_menu_row_height = 32;
constexpr int tray_menu_edge_padding = 4;
constexpr int tray_menu_icon_size = 16;
constexpr std::uint32_t tray_item_magic = 0x53544D49;  // STMI

enum class TrayMenuIcon {
    none,
    check,
    capture,
    text,
    restore,
    settings,
    update,
    restart,
    exit,
};

struct TrayMenuItem {
    std::uint32_t magic{tray_item_magic};
    UINT command{};
    TrayMenuIcon icon{TrayMenuIcon::none};
    std::wstring label;
    std::wstring secondary;
    std::wstring accessible_text;
    UINT dpi{96};
    HFONT font{};
    bool enabled{true};
    bool separator{};
    bool first{};
    bool last{};
};

int tray_scaled(int logical, UINT dpi) noexcept {
    return std::max(1, MulDiv(logical, static_cast<int>(dpi), 96));
}

UINT tray_menu_dpi(POINT location) noexcept {
    const HMONITOR monitor = MonitorFromPoint(location, MONITOR_DEFAULTTONEAREST);
    UINT dpi_x = 96;
    UINT dpi_y = 96;
    if (monitor && SUCCEEDED(GetDpiForMonitor(
            monitor, MDT_EFFECTIVE_DPI, &dpi_x, &dpi_y))) {
        return std::max<UINT>(96, dpi_x);
    }
    return std::max<UINT>(96, GetDpiForSystem());
}

HFONT create_tray_menu_font(UINT dpi) noexcept {
    return CreateFontW(-MulDiv(9, static_cast<int>(dpi), 72), 0, 0, 0, FW_NORMAL,
                       FALSE, FALSE, FALSE, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                       CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                       DEFAULT_PITCH | FF_DONTCARE, L"Microsoft YaHei UI");
}

void CALLBACK style_tray_menu_window(HWINEVENTHOOK, DWORD event, HWND window,
                                     LONG, LONG, DWORD, DWORD) noexcept {
    if (event != EVENT_SYSTEM_MENUPOPUPSTART || !window) return;
    wchar_t class_name[16]{};
    if (!GetClassNameW(window, class_name, static_cast<int>(std::size(class_name))) ||
        wcscmp(class_name, L"#32768") != 0) {
        return;
    }

    SetWindowTheme(window, L"DarkMode_Explorer", nullptr);
    constexpr DWORD immersive_dark_mode = 20;
    constexpr DWORD window_corner_preference = 33;
    constexpr DWORD border_color = 34;
    constexpr int round = 2;
    BOOL dark = TRUE;
    DwmSetWindowAttribute(window, immersive_dark_mode, &dark, sizeof(dark));
    DwmSetWindowAttribute(window, window_corner_preference, &round, sizeof(round));
    const COLORREF border = tray_color_border;
    DwmSetWindowAttribute(window, border_color, &border, sizeof(border));
}

bool is_tray_menu_item(ULONG_PTR data) noexcept {
    const auto* item = reinterpret_cast<const TrayMenuItem*>(data);
    return item && item->magic == tray_item_magic;
}

int CALLBACK find_tray_icon_font(const LOGFONTW*, const TEXTMETRICW*, DWORD,
                                 LPARAM data) {
    *reinterpret_cast<bool*>(data) = true;
    return 0;
}

bool tray_icon_font_available(HDC dc, const wchar_t* face) {
    LOGFONTW query{};
    query.lfCharSet = DEFAULT_CHARSET;
    if (wcsncpy_s(query.lfFaceName, LF_FACESIZE, face, _TRUNCATE) != 0) {
        return false;
    }
    bool found = false;
    EnumFontFamiliesExW(dc, &query, find_tray_icon_font,
                        reinterpret_cast<LPARAM>(&found), 0);
    return found;
}

const wchar_t* tray_icon_font(HDC dc) {
    static int fluent_available = -1;
    if (fluent_available < 0) {
        fluent_available = tray_icon_font_available(dc, L"Segoe Fluent Icons") ? 1 : 0;
    }
    return fluent_available ? L"Segoe Fluent Icons" : L"Segoe MDL2 Assets";
}

void draw_tray_menu_icon(HDC dc, const RECT& bounds, TrayMenuIcon icon,
                         COLORREF color, UINT dpi) {
    if (icon == TrayMenuIcon::none) return;
    wchar_t glyph = 0;
    switch (icon) {
    case TrayMenuIcon::check:
        glyph = static_cast<wchar_t>(0xE73E);  // CheckMark
        break;
    case TrayMenuIcon::capture:
        glyph = static_cast<wchar_t>(0xE7A8);  // Crop
        break;
    case TrayMenuIcon::text:
        glyph = static_cast<wchar_t>(0xE8D2);  // Edit
        break;
    case TrayMenuIcon::restore:
        glyph = static_cast<wchar_t>(0xE890);  // View
        break;
    case TrayMenuIcon::settings:
        glyph = static_cast<wchar_t>(0xE713);  // Settings
        break;
    case TrayMenuIcon::update:
        glyph = static_cast<wchar_t>(0xE896);  // Download
        break;
    case TrayMenuIcon::restart:
        glyph = static_cast<wchar_t>(0xE72C);  // Refresh
        break;
    case TrayMenuIcon::exit:
        glyph = static_cast<wchar_t>(0xE7E8);  // PowerButton
        break;
    case TrayMenuIcon::none:
        break;
    }
    if (!glyph) return;

    const int icon_width = static_cast<int>(bounds.right - bounds.left);
    const int icon_height = static_cast<int>(bounds.bottom - bounds.top);
    const int font_pixels = std::max(tray_scaled(14, dpi),
                                     std::min(icon_width, icon_height));
    HFONT font = CreateFontW(-font_pixels, 0, 0, 0, FW_NORMAL,
                             FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                             OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                             CLEARTYPE_QUALITY, DEFAULT_PITCH | FF_DONTCARE,
                             tray_icon_font(dc));
    if (!font) return;

    const int saved = SaveDC(dc);
    SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, color);
    RECT text_bounds = bounds;
    DrawTextW(dc, &glyph, 1, &text_bounds,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    RestoreDC(dc, saved);
    DeleteObject(font);
}

bool measure_tray_menu_item(MEASUREITEMSTRUCT& measure) {
    if (measure.CtlType != ODT_MENU || !is_tray_menu_item(measure.itemData)) return false;
    const auto& item = *reinterpret_cast<const TrayMenuItem*>(measure.itemData);
    const int top_padding = item.first ? tray_menu_edge_padding : 0;
    const int bottom_padding = item.last ? tray_menu_edge_padding : 0;
    if (item.separator) {
        measure.itemWidth = static_cast<UINT>(tray_scaled(tray_menu_min_width, item.dpi));
        measure.itemHeight = static_cast<UINT>(tray_scaled(9 + top_padding + bottom_padding,
                                                           item.dpi));
        return true;
    }

    SIZE label_size{};
    SIZE secondary_size{};
    HDC dc = GetDC(nullptr);
    HGDIOBJ previous_font = nullptr;
    if (dc && item.font) previous_font = SelectObject(dc, item.font);
    if (dc) {
        GetTextExtentPoint32W(dc, item.label.c_str(), static_cast<int>(item.label.size()),
                              &label_size);
        GetTextExtentPoint32W(dc, item.secondary.c_str(),
                              static_cast<int>(item.secondary.size()), &secondary_size);
        if (previous_font) SelectObject(dc, previous_font);
        ReleaseDC(nullptr, dc);
    }

    const int secondary_gap = item.secondary.empty() ? 0 : tray_scaled(22, item.dpi);
    const int measured_width = tray_scaled(54, item.dpi) + label_size.cx +
                               secondary_gap + secondary_size.cx + tray_scaled(14, item.dpi);
    measure.itemWidth = static_cast<UINT>(std::max(
        tray_scaled(tray_menu_min_width, item.dpi), measured_width));
    measure.itemHeight = static_cast<UINT>(tray_scaled(
        tray_menu_row_height + top_padding + bottom_padding,
                                                       item.dpi));
    return true;
}

bool draw_tray_menu_item(const DRAWITEMSTRUCT& draw) {
    if (draw.CtlType != ODT_MENU || !is_tray_menu_item(draw.itemData)) return false;
    const auto& item = *reinterpret_cast<const TrayMenuItem*>(draw.itemData);
    const bool disabled = !item.enabled || (draw.itemState & ODS_DISABLED) != 0;
    const bool selected = !disabled && (draw.itemState & ODS_SELECTED) != 0;

    HBRUSH background = CreateSolidBrush(tray_color_background);
    if (background) {
        FillRect(draw.hDC, &draw.rcItem, background);
        DeleteObject(background);
    }

    RECT content = draw.rcItem;
    if (item.first) content.top += tray_scaled(tray_menu_edge_padding, item.dpi);
    if (item.last) content.bottom -= tray_scaled(tray_menu_edge_padding, item.dpi);

    if (item.separator) {
        const int middle = content.top + (content.bottom - content.top) / 2;
        HPEN pen = CreatePen(PS_SOLID, tray_scaled(1, item.dpi), tray_color_separator);
        if (pen) {
            const auto previous = SelectObject(draw.hDC, pen);
            MoveToEx(draw.hDC, content.left + tray_scaled(14, item.dpi), middle, nullptr);
            LineTo(draw.hDC, content.right - tray_scaled(14, item.dpi), middle);
            SelectObject(draw.hDC, previous);
            DeleteObject(pen);
        }
        return true;
    }

    if (selected) {
        RECT highlight = content;
        InflateRect(&highlight, -tray_scaled(3, item.dpi), -tray_scaled(1, item.dpi));
        HBRUSH accent_brush = CreateSolidBrush(tray_color_hover);
        HPEN accent_pen = CreatePen(PS_SOLID, 1, tray_color_hover);
        if (accent_brush && accent_pen) {
            const auto old_brush = SelectObject(draw.hDC, accent_brush);
            const auto old_pen = SelectObject(draw.hDC, accent_pen);
            RoundRect(draw.hDC, highlight.left, highlight.top, highlight.right, highlight.bottom,
                      tray_scaled(8, item.dpi), tray_scaled(8, item.dpi));
            SelectObject(draw.hDC, old_pen);
            SelectObject(draw.hDC, old_brush);
        }
        if (accent_pen) DeleteObject(accent_pen);
        if (accent_brush) DeleteObject(accent_brush);
    }

    const COLORREF foreground = selected ? RGB(245, 247, 248)
        : disabled ? tray_color_text_faint : tray_color_text;
    const COLORREF secondary_color = selected ? RGB(198, 204, 211)
        : disabled ? tray_color_text_faint : tray_color_text_dim;
    const int icon_size = tray_scaled(tray_menu_icon_size, item.dpi);
    RECT icon_bounds{
        content.left + tray_scaled(14, item.dpi),
        content.top + (content.bottom - content.top - icon_size) / 2,
        content.left + tray_scaled(14, item.dpi) + icon_size,
        content.top + (content.bottom - content.top + icon_size) / 2,
    };
    const auto icon = (draw.itemState & ODS_CHECKED) != 0
        ? TrayMenuIcon::check : item.icon;
    draw_tray_menu_icon(draw.hDC, icon_bounds, icon,
                        foreground, item.dpi);

    const int saved = SaveDC(draw.hDC);
    SetBkMode(draw.hDC, TRANSPARENT);
    if (item.font) SelectObject(draw.hDC, item.font);

    RECT label_rect = content;
    label_rect.left += tray_scaled(43, item.dpi);
    label_rect.right -= tray_scaled(14, item.dpi);
    if (!item.secondary.empty()) {
        SIZE secondary_size{};
        GetTextExtentPoint32W(draw.hDC, item.secondary.c_str(),
                              static_cast<int>(item.secondary.size()), &secondary_size);
        label_rect.right -= secondary_size.cx + tray_scaled(22, item.dpi);
        RECT secondary_rect = content;
        secondary_rect.left = label_rect.right + tray_scaled(12, item.dpi);
        secondary_rect.right -= tray_scaled(14, item.dpi);
        SetTextColor(draw.hDC, secondary_color);
        DrawTextW(draw.hDC, item.secondary.c_str(), static_cast<int>(item.secondary.size()),
                  &secondary_rect, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX);
    }
    SetTextColor(draw.hDC, foreground);
    DrawTextW(draw.hDC, item.label.c_str(), static_cast<int>(item.label.size()), &label_rect,
              DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS | DT_NOPREFIX);
    RestoreDC(draw.hDC, saved);
    return true;
}

void tray_menu_layout_self_test() {
    static_assert(tray_menu_icon_size <= tray_menu_row_height / 2);
    TrayMenuItem item;
    item.command = command_capture;
    item.icon = TrayMenuIcon::capture;
    item.label = L"开始框选翻译";
    item.secondary = L"Ctrl+Alt+Q";
    item.dpi = 96;
    item.font = create_tray_menu_font(item.dpi);
    item.first = true;

    MEASUREITEMSTRUCT measured{};
    measured.CtlType = ODT_MENU;
    measured.itemData = reinterpret_cast<ULONG_PTR>(&item);
    if (!measure_tray_menu_item(measured) ||
        measured.itemHeight != static_cast<UINT>(tray_scaled(
            tray_menu_row_height + tray_menu_edge_padding, item.dpi)) ||
        measured.itemWidth < static_cast<UINT>(
            tray_scaled(tray_menu_min_width, item.dpi))) {
        if (item.font) DeleteObject(item.font);
        throw AppError("tray menu layout self-test failed");
    }

    HDC screen_dc = GetDC(nullptr);
    HDC dc = CreateCompatibleDC(screen_dc);
    HBITMAP bitmap = dc && screen_dc ? CreateCompatibleBitmap(
        screen_dc, static_cast<int>(measured.itemWidth),
        static_cast<int>(measured.itemHeight)) : nullptr;
    if (screen_dc) ReleaseDC(nullptr, screen_dc);
    HGDIOBJ previous_bitmap = bitmap && dc ? SelectObject(dc, bitmap) : nullptr;
    DRAWITEMSTRUCT drawn{};
    drawn.CtlType = ODT_MENU;
    drawn.itemState = ODS_SELECTED;
    drawn.hDC = dc;
    drawn.rcItem = {0, 0, static_cast<LONG>(measured.itemWidth),
                    static_cast<LONG>(measured.itemHeight)};
    drawn.itemData = reinterpret_cast<ULONG_PTR>(&item);
    const bool rendered = dc && bitmap && draw_tray_menu_item(drawn);
    if (previous_bitmap) SelectObject(dc, previous_bitmap);
    if (bitmap) DeleteObject(bitmap);
    if (dc) DeleteDC(dc);
    if (item.font) DeleteObject(item.font);
    if (!rendered) throw AppError("tray menu render self-test failed");
}

HICON load_accent_icon(HINSTANCE instance, COLORREF accent) {
    constexpr int icon_size = 32;
    HICON source = static_cast<HICON>(LoadImageW(
        instance, MAKEINTRESOURCEW(IDI_APP_ICON), IMAGE_ICON,
        icon_size, icon_size, LR_DEFAULTCOLOR));
    if (!source) return nullptr;

    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = icon_size;
    info.bmiHeader.biHeight = -icon_size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* raw_bits = nullptr;
    HDC screen = GetDC(nullptr);
    HBITMAP color = screen
        ? CreateDIBSection(screen, &info, DIB_RGB_COLORS, &raw_bits, nullptr, 0)
        : nullptr;
    HDC memory = screen ? CreateCompatibleDC(screen) : nullptr;
    if (screen) ReleaseDC(nullptr, screen);
    if (!color || !raw_bits || !memory) {
        if (memory) DeleteDC(memory);
        if (color) DeleteObject(color);
        return source;
    }

    std::memset(raw_bits, 0, icon_size * icon_size * 4);
    const HGDIOBJ previous = SelectObject(memory, color);
    if (!previous || previous == HGDI_ERROR) {
        DeleteDC(memory);
        DeleteObject(color);
        return source;
    }
    const BOOL drawn = DrawIconEx(memory, 0, 0, source, icon_size, icon_size,
                                  0, nullptr, DI_NORMAL);
    SelectObject(memory, previous);
    DeleteDC(memory);
    if (!drawn) {
        DeleteObject(color);
        return source;
    }

    auto* pixels = static_cast<BYTE*>(raw_bits);
    constexpr int original_accent_peak = 199;
    const std::array<int, 3> replacement{
        GetBValue(accent), GetGValue(accent), GetRValue(accent),
    };
    for (int index = 0; index < icon_size * icon_size; ++index) {
        BYTE* pixel = pixels + index * 4;
        const int alpha = pixel[3];
        if (alpha == 0) continue;
        const int red = pixel[2];
        const int green = pixel[1];
        const int blue = pixel[0];
        const int maximum = std::max({red, green, blue});
        const int minimum = std::min({red, green, blue});
        if (maximum - minimum <= 18 || green < red || green < blue) continue;

        const bool premultiplied = maximum <= alpha + 2;
        const int straight_peak = premultiplied && alpha < 255
            ? std::min(255, maximum * 255 / std::max(1, alpha))
            : maximum;
        for (int channel = 0; channel < 3; ++channel) {
            int value = std::clamp(
                replacement[static_cast<std::size_t>(channel)] * straight_peak
                    / original_accent_peak,
                0, 255);
            if (premultiplied && alpha < 255) value = value * alpha / 255;
            pixel[channel] = static_cast<BYTE>(value);
        }
    }

    std::array<BYTE, icon_size * icon_size / 8> mask_bits{};
    HBITMAP mask = CreateBitmap(icon_size, icon_size, 1, 1, mask_bits.data());
    ICONINFO icon_info{};
    icon_info.fIcon = TRUE;
    icon_info.hbmColor = color;
    icon_info.hbmMask = mask;
    HICON tinted = mask ? CreateIconIndirect(&icon_info) : nullptr;
    if (mask) DeleteObject(mask);
    DeleteObject(color);
    if (!tinted) return source;
    DestroyIcon(source);
    return tinted;
}

bool icon_contains_red_accent(HICON icon) {
    if (!icon) return false;
    constexpr int icon_size = 32;
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = icon_size;
    info.bmiHeader.biHeight = -icon_size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    void* raw_bits = nullptr;
    HDC screen = GetDC(nullptr);
    HBITMAP bitmap = screen
        ? CreateDIBSection(screen, &info, DIB_RGB_COLORS, &raw_bits, nullptr, 0)
        : nullptr;
    HDC memory = screen ? CreateCompatibleDC(screen) : nullptr;
    if (screen) ReleaseDC(nullptr, screen);
    if (!bitmap || !raw_bits || !memory) {
        if (memory) DeleteDC(memory);
        if (bitmap) DeleteObject(bitmap);
        return false;
    }

    std::memset(raw_bits, 0, icon_size * icon_size * 4);
    const HGDIOBJ previous = SelectObject(memory, bitmap);
    const bool selected = previous && previous != HGDI_ERROR;
    const bool drawn = selected && DrawIconEx(memory, 0, 0, icon, icon_size, icon_size,
                                              0, nullptr, DI_NORMAL) != FALSE;
    if (selected) SelectObject(memory, previous);
    DeleteDC(memory);

    bool found = false;
    if (drawn) {
        const auto* pixels = static_cast<const BYTE*>(raw_bits);
        int matching_pixels = 0;
        for (int index = 0; index < icon_size * icon_size; ++index) {
            const BYTE* pixel = pixels + index * 4;
            if (pixel[3] >= 32 && pixel[2] >= 48
                && pixel[2] >= pixel[1] + 24 && pixel[2] >= pixel[0] + 16) {
                if (++matching_pixels >= 8) {
                    found = true;
                    break;
                }
            }
        }
    }
    DeleteObject(bitmap);
    return found;
}

}  // namespace

AppHost::AppHost(HINSTANCE instance)
    : instance_(instance), settings_(instance, config_), updates_(instance, config_),
      overlay_(instance), result_(instance) {
    create_message_window();
    pipeline_ = std::make_unique<PipelineController>(window_);
    text_session_ = std::make_unique<TextTranslationSession>(config_, *pipeline_);
    quick_window_ = std::make_unique<QuickTranslateWindow>(instance_, config_, *text_session_);
    TextTranslationCallbacks text_callbacks;
    text_callbacks.input_changed = [this](std::wstring value) {
        on_text_input_changed(std::move(value));
    };
    text_callbacks.target_changed = [this](std::wstring value) {
        on_text_target_changed(std::move(value));
    };
    text_callbacks.composition_changed = [this](bool composing) {
        on_text_composition_changed(composing);
    };
    text_callbacks.translate_now = [this] { schedule_text_translation(true); };
    text_callbacks.clear = [this] {
        KillTimer(window_, text_translate_timer);
        if (text_session_) text_session_->clear();
    };
    text_callbacks.copy = [this] {
        if (text_session_) copy_to_clipboard(text_session_->output());
    };
    text_callbacks.open_settings = [this] { open_translation_settings(); };
    settings_.attach_text_translation(*text_session_, text_callbacks);
    quick_window_->set_callbacks(std::move(text_callbacks));
    text_session_->set_changed_callback([this] { refresh_text_translation_views(); });
    result_.set_copy_callback([this](std::wstring_view text) { copy_to_clipboard(text); });
    result_.set_retry_callback([this] { retry_translation(); });
    result_.set_edit_callback([this] { open_block_editor(); });
    result_.set_closed_callback([this] {
        BlockEditor::close_active();
        KillTimer(window_, recapture_timer);
        pending_recapture_.reset();
        current_request_ = 0;
        block_translation_request_.reset();
        last_image_.reset();
        last_blocks_.clear();
        last_result_.reset();
        if (pipeline_) pipeline_->cancel_lane(PipelineLane::visual);
    });
    result_.set_recapture_callback([this](const RECT& rect) { request_recapture(rect); });
    settings_.set_hotkeys_changed_callback([this] { return apply_hotkeys(true); });
    settings_.set_restart_callback([this] { restart(); });
    settings_.set_settings_changed_callback([this] {
        refresh_tray_tooltip();
        refresh_tray_icon();
        updates_.refresh_appearance();
        if (quick_window_) quick_window_->refresh_appearance();
        if (text_session_ && text_session_->configuration_changed()) {
            schedule_text_translation(true);
        }
    });
    updates_.set_quit_callback([this] { quit(); });
    updates_.set_update_available_callback(
        [this](const UpdateInfo& update) { handle_update_available(update); });
    if (!PostMessageW(window_, initialize_message, 0, 0)) {
        throw_last_error("queue application initialization");
    }
}

AppHost::~AppHost() {
    shutting_down_ = true;
    BlockEditor::close_active();
    result_.set_closed_callback({});
    result_.close();
    overlay_.close();
    KillTimer(window_, text_translate_timer);
    if (text_session_) text_session_->set_changed_callback({});
    settings_.detach_text_translation();
    quick_window_.reset();
    text_session_.reset();
    pipeline_.reset();
    if (silent_update_thread_.joinable()) {
        silent_update_thread_.request_stop();
        silent_update_thread_.join();
    }
    remove_tray_icon();
    if (window_) {
        UnregisterHotKey(window_, hotkey_capture);
        UnregisterHotKey(window_, hotkey_toggle);
        UnregisterHotKey(window_, hotkey_text_translate);
        DestroyWindow(window_);
        window_ = nullptr;
    }
}

void AppHost::create_message_window() {
    WNDCLASSEXW description{};
    description.cbSize = sizeof(description);
    description.lpfnWndProc = &AppHost::window_proc;
    description.hInstance = instance_;
    description.hIcon = LoadIconW(instance_, MAKEINTRESOURCEW(IDI_APP_ICON));
    description.lpszClassName = window_class_name();
    if (!RegisterClassExW(&description) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        throw_last_error("register application window");
    }
    window_ = CreateWindowExW(0, window_class_name(), L"划词截屏翻译", 0,
                              0, 0, 0, 0, nullptr, nullptr, instance_, this);
    if (!window_) throw_last_error("create application window");
    taskbar_created_message_ = RegisterWindowMessageW(L"TaskbarCreated");
}

void AppHost::add_tray_icon() {
    if (icon_) {
        DestroyIcon(icon_);
        icon_ = nullptr;
    }

    const auto accent_text = config_.string(L"appearance.accent", L"#28C76F");
    HICON icon = load_accent_icon(instance_, parse_rgb_color(accent_text));
    if (!icon) throw_last_error("load notification icon");

    NOTIFYICONDATAW tray{};
    tray.cbSize = sizeof(tray);
    tray.hWnd = window_;
    tray.uID = 1;
    tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    tray.uCallbackMessage = tray_message;
    tray.hIcon = icon;
    const auto capture = config_.string(L"hotkey", L"Ctrl+Alt+Q");
    const auto toggle = config_.string(L"hotkey_toggle", L"Ctrl+Alt+W");
    const auto text = config_.string(L"hotkey_text_translate", L"Ctrl+Alt+Space");
    const auto tip = L"划词截屏翻译\n框选：" + capture + L"\n快速翻译：" + text +
                     L"\n收起 / 显示：" + toggle;
    wcsncpy_s(tray.szTip, tip.c_str(), _TRUNCATE);
    if (!Shell_NotifyIconW(NIM_ADD, &tray)) {
        DestroyIcon(icon);
        throw AppError("cannot create notification icon");
    }
    tray.uVersion = NOTIFYICON_VERSION_4;
    Shell_NotifyIconW(NIM_SETVERSION, &tray);
    tray_ = tray;
    icon_ = icon;
    tray_accent_ = accent_text;
}

void AppHost::remove_tray_icon() {
    if (tray_.hWnd) {
        Shell_NotifyIconW(NIM_DELETE, &tray_);
        tray_.hWnd = nullptr;
    }
    if (icon_) {
        DestroyIcon(icon_);
        icon_ = nullptr;
    }
    tray_accent_.clear();
}

void AppHost::refresh_tray_icon() {
    if (!tray_.hWnd) return;
    const auto accent_text = config_.string(L"appearance.accent", L"#28C76F");
    if (icon_ && accent_text == tray_accent_) return;
    HICON replacement = load_accent_icon(instance_, parse_rgb_color(accent_text));
    if (!replacement) return;
    const HICON previous = icon_;
    tray_.uFlags = NIF_ICON;
    tray_.hIcon = replacement;
    if (Shell_NotifyIconW(NIM_MODIFY, &tray_)) {
        icon_ = replacement;
        tray_accent_ = accent_text;
        if (previous) DestroyIcon(previous);
    } else {
        tray_.hIcon = previous;
        DestroyIcon(replacement);
    }
}

void AppHost::refresh_tray_tooltip() {
    if (!tray_.hWnd) return;
    const auto capture = config_.string(L"hotkey", L"Ctrl+Alt+Q");
    const auto toggle = config_.string(L"hotkey_toggle", L"Ctrl+Alt+W");
    const auto text = config_.string(L"hotkey_text_translate", L"Ctrl+Alt+Space");
    const auto tip = L"划词截屏翻译\n框选：" + capture + L"\n快速翻译：" + text +
                     L"\n收起 / 显示：" + toggle;
    tray_.uFlags = NIF_TIP;
    wcsncpy_s(tray_.szTip, tip.c_str(), _TRUNCATE);
    Shell_NotifyIconW(NIM_MODIFY, &tray_);
}

void AppHost::notify(std::wstring_view title, std::wstring_view message) {
    if (!tray_.hWnd) return;
    tray_.uFlags = NIF_INFO;
    wcsncpy_s(tray_.szInfoTitle, std::wstring(title).c_str(), _TRUNCATE);
    wcsncpy_s(tray_.szInfo, std::wstring(message).c_str(), _TRUNCATE);
    tray_.dwInfoFlags = NIIF_USER;
    tray_.hBalloonIcon = icon_;
    tray_.uTimeout = 3000;
    Shell_NotifyIconW(NIM_MODIFY, &tray_);
}

void AppHost::show_tray_menu(POINT location) {
    HMENU menu = CreatePopupMenu();
    if (!menu) return;

    const UINT dpi = tray_menu_dpi(location);
    HFONT menu_font = create_tray_menu_font(dpi);
    const bool owns_font = menu_font != nullptr;
    if (!menu_font) menu_font = static_cast<HFONT>(GetStockObject(DEFAULT_GUI_FONT));
    HBRUSH menu_background = CreateSolidBrush(tray_color_background);
    MENUINFO menu_info{};
    menu_info.cbSize = sizeof(menu_info);
    menu_info.fMask = MIM_STYLE | (menu_background ? MIM_BACKGROUND : 0);
    menu_info.dwStyle = MNS_NOCHECK;
    menu_info.hbrBack = menu_background;
    SetMenuInfo(menu, &menu_info);

    const auto capture = config_.string(L"hotkey", L"Ctrl+Alt+Q");
    const auto toggle = config_.string(L"hotkey_toggle", L"Ctrl+Alt+W");
    const bool can_restore = last_image_ && !result_.visible();
    std::vector<TrayMenuItem> items;
    items.reserve(9);
    const auto add_item = [&](UINT command, TrayMenuIcon icon, std::wstring label,
                              std::wstring secondary = {}, bool enabled = true) {
        TrayMenuItem item;
        item.command = command;
        item.icon = icon;
        item.label = std::move(label);
        item.secondary = std::move(secondary);
        item.dpi = dpi;
        item.font = menu_font;
        item.enabled = enabled;
        items.push_back(std::move(item));
    };
    add_item(command_capture, TrayMenuIcon::capture, L"开始框选翻译", capture);
    add_item(command_text_translate, TrayMenuIcon::text, L"文字翻译…");
    add_item(command_restore, TrayMenuIcon::restore, L"显示上次译文", toggle, can_restore);
    TrayMenuItem separator;
    separator.separator = true;
    separator.enabled = false;
    separator.dpi = dpi;
    separator.font = menu_font;
    items.push_back(std::move(separator));
    add_item(command_settings, TrayMenuIcon::settings, L"设置…");
    if (available_update_version_.empty()) {
        add_item(command_updates, TrayMenuIcon::update, L"检查更新…");
    } else {
        add_item(command_updates, TrayMenuIcon::update, L"发现新版本",
                 available_update_version_ + L"…");
    }
    add_item(command_restart, TrayMenuIcon::restart, L"重启");
    add_item(command_exit, TrayMenuIcon::exit, L"退出");
    items.front().first = true;
    items.back().last = true;

    for (std::size_t index = 0; index < items.size(); ++index) {
        auto& item = items[index];
        item.accessible_text = item.label;
        if (!item.secondary.empty()) item.accessible_text += L"\t" + item.secondary;

        MENUITEMINFOW menu_item{};
        menu_item.cbSize = sizeof(menu_item);
        menu_item.fMask = MIIM_FTYPE | MIIM_STATE | MIIM_ID | MIIM_DATA | MIIM_STRING;
        menu_item.fType = MFT_OWNERDRAW | (item.separator ? MFT_SEPARATOR : 0);
        menu_item.fState = item.enabled ? MFS_ENABLED : MFS_DISABLED;
        menu_item.wID = item.command;
        menu_item.dwItemData = reinterpret_cast<ULONG_PTR>(&item);
        menu_item.dwTypeData = item.accessible_text.data();
        menu_item.cch = static_cast<UINT>(item.accessible_text.size());
        InsertMenuItemW(menu, static_cast<UINT>(index), TRUE, &menu_item);
    }

    SetForegroundWindow(window_);
    HWINEVENTHOOK menu_hook = SetWinEventHook(
        EVENT_SYSTEM_MENUPOPUPSTART, EVENT_SYSTEM_MENUPOPUPSTART, nullptr,
        &style_tray_menu_window, GetCurrentProcessId(), GetCurrentThreadId(),
        WINEVENT_OUTOFCONTEXT);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                   location.x, location.y, 0, window_, nullptr);
    if (menu_hook) UnhookWinEvent(menu_hook);
    DestroyMenu(menu);
    if (menu_background) DeleteObject(menu_background);
    if (owns_font) DeleteObject(menu_font);
    PostMessageW(window_, WM_NULL, 0, 0);
}

bool AppHost::apply_hotkeys(bool announce_changes) {
    // Release all registrations first so exchanging shortcuts is valid.
    const auto unregister_all = [this] {
        UnregisterHotKey(window_, hotkey_capture);
        UnregisterHotKey(window_, hotkey_toggle);
        UnregisterHotKey(window_, hotkey_text_translate);
    };
    unregister_all();
    bool success = true;
    std::wstring error;
    const auto capture_spec = config_.string(L"hotkey", L"Ctrl+Alt+Q");
    const auto toggle_spec = config_.string(L"hotkey_toggle", L"Ctrl+Alt+W");
    const auto text_spec = config_.string(
        L"hotkey_text_translate", L"Ctrl+Alt+Space");
    const bool capture_registered = register_hotkey(
        window_, hotkey_capture, capture_spec, &error);
    if (!capture_registered) {
        success = false;
        notify(L"划词截屏翻译 · 框选翻译快捷键未生效", error);
    }
    error.clear();
    const bool toggle_registered = register_hotkey(
        window_, hotkey_toggle, toggle_spec, &error);
    if (!toggle_registered) {
        success = false;
        notify(L"划词截屏翻译 · 收起 / 显示快捷键未生效", error);
    }
    error.clear();
    const bool text_registered = register_hotkey(
        window_, hotkey_text_translate, text_spec, &error);
    if (!text_registered) {
        success = false;
        notify(L"划词截屏翻译 · 快速翻译快捷键未生效", error);
    }
    if (!success) {
        unregister_all();
        bool restored = true;
        const auto restore = [&](int id, const std::wstring& spec) {
            if (spec.empty()) return;
            std::wstring restore_error;
            if (!register_hotkey(window_, id, spec, &restore_error)) restored = false;
        };
        restore(hotkey_capture, registered_capture_hotkey_);
        restore(hotkey_toggle, registered_toggle_hotkey_);
        restore(hotkey_text_translate, registered_text_hotkey_);
        if (!restored) {
            notify(L"划词截屏翻译 · 快捷键恢复失败",
                   L"原快捷键也被其他程序占用，请在设置中重新选择");
        }
        return false;
    }
    {
        const auto old_capture = registered_capture_hotkey_;
        const auto old_toggle = registered_toggle_hotkey_;
        const auto old_text = registered_text_hotkey_;
        registered_capture_hotkey_ = capture_spec;
        registered_toggle_hotkey_ = toggle_spec;
        registered_text_hotkey_ = text_spec;
        refresh_tray_tooltip();
        if (announce_changes && !old_capture.empty() && old_capture != capture_spec) {
            notify(L"划词截屏翻译", L"框选翻译快捷键已改为 " + capture_spec);
        }
        if (announce_changes && !old_toggle.empty() && old_toggle != toggle_spec) {
            notify(L"划词截屏翻译", L"收起 / 显示快捷键已改为 " + toggle_spec);
        }
        if (announce_changes && !old_text.empty() && old_text != text_spec) {
            notify(L"划词截屏翻译", L"快速翻译快捷键已改为 " + text_spec);
        }
    }
    return true;
}

void AppHost::start_capture() {
    if (overlay_.active() || shutting_down_) return;
    if (quick_window_) quick_window_->hide();
    BlockEditor::close_active();
    KillTimer(window_, recapture_timer);
    pending_recapture_.reset();
    if (pipeline_) pipeline_->cancel_lane(PipelineLane::visual);
    current_request_ = 0;
    block_translation_request_.reset();
    result_.close();
    last_image_.reset();
    last_blocks_.clear();
    last_result_.reset();
    KillTimer(window_, capture_timer);
    SetTimer(window_, capture_timer, 30, nullptr);
}

void AppHost::begin_capture_now() {
    try {
        overlay_.start(config_.string(L"appearance.accent", L"#28C76F"),
            [this](const RECT& rect, PixelBuffer image) noexcept {
                try {
                    on_selected(rect, std::move(image));
                } catch (const std::exception& error) {
                    notify(L"划词截屏翻译", L"无法创建译文窗口：" + utf8_to_wide(error.what()));
                } catch (...) {
                    notify(L"划词截屏翻译", L"无法创建译文窗口：未知错误");
                }
            },
            [] {});
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"无法截取屏幕：" + utf8_to_wide(error.what()));
    }
}

void AppHost::on_selected(const RECT& rect, PixelBuffer image) {
    auto shared = std::make_shared<PixelBuffer>(std::move(image));
    start_pipeline(shared, rect);
    // Commit the replacement surface before SelectionOverlay destroys the
    // frozen desktop. The pixels under the selected area stay identical.
    DwmFlush();
}

void AppHost::start_pipeline(std::shared_ptr<const PixelBuffer> image, const RECT& rect) {
    last_image_ = image;
    last_rect_ = rect;
    result_.show_loading(rect, image, result_appearance());
    try {
        current_request_ = pipeline_->schedule(image, PipelineOptions::from_config(config_));
    } catch (const std::exception& error) {
        result_.set_error(L"启动翻译失败：" + utf8_to_wide(error.what()));
    }
}

void AppHost::process_pipeline_completions() {
    if (!pipeline_) return;
    for (auto& completion : pipeline_->take_completions()) {
        if (completion.lane == PipelineLane::text) {
            if (text_session_) text_session_->handle_completion(std::move(completion));
            continue;
        }
        if (block_translation_request_ &&
            completion.request_id == block_translation_request_->request_id) {
            process_block_translation(std::move(completion));
            continue;
        }
        if (!current_request_ || completion.request_id != current_request_) continue;
        if (completion.result) {
            const auto auto_copy = config_.boolean(L"appearance.auto_copy", true);
            const auto copy = completion.result->plain_text;
            last_result_ = *completion.result;
            last_blocks_.clear();
            last_blocks_.reserve(completion.result->blocks.size());
            for (const auto& item : completion.result->blocks) {
                last_blocks_.push_back(item.block);
            }
            result_.set_result(std::move(*completion.result));
            if (auto_copy && !copy.empty()) copy_to_clipboard(copy);
        } else {
            result_.set_error(completion.error.empty() ? L"识别或翻译失败" : completion.error);
        }
    }
}

void AppHost::open_text_translation() {
    if (shutting_down_) return;
    try {
        settings_.show_text_translation(window_);
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"无法打开文字翻译：" + utf8_to_wide(error.what()));
    }
}

void AppHost::toggle_quick_translation() {
    if (!quick_window_ || shutting_down_ || overlay_.active()) return;
    try {
        quick_window_->toggle(window_);
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"无法打开快速翻译：" + utf8_to_wide(error.what()));
    }
}

void AppHost::on_text_input_changed(std::wstring value) {
    if (!text_session_ || !text_session_->set_input(std::move(value))) return;
    schedule_text_translation(false);
}

void AppHost::on_text_target_changed(std::wstring value) {
    if (!text_session_ || !text_session_->set_target(std::move(value))) return;
    schedule_text_translation(true);
}

void AppHost::on_text_composition_changed(bool composing) {
    text_composing_ = composing;
    KillTimer(window_, text_translate_timer);
    if (!composing) schedule_text_translation(false);
}

void AppHost::schedule_text_translation(bool immediate) {
    KillTimer(window_, text_translate_timer);
    if (!text_session_ || text_composing_ || !text_session_->can_submit()) return;
    if (immediate) {
        text_session_->submit();
    } else {
        SetTimer(window_, text_translate_timer, 500, nullptr);
    }
}

void AppHost::refresh_text_translation_views() {
    settings_.refresh_text_translation();
    if (quick_window_) quick_window_->refresh();
}

void AppHost::toggle_result() {
    if (result_.visible()) {
        result_.minimize();
        const auto key = config_.string(L"hotkey_toggle", L"Ctrl+Alt+W");
        notify(L"划词截屏翻译",
               L"译文已收起，按 " + key + L" 或从托盘菜单「显示上次译文」叫回来");
    } else if (last_image_) {
        result_.restore();
    } else {
        notify(L"划词截屏翻译", L"现在没有译文可以显示");
    }
}

void AppHost::copy_to_clipboard(std::wstring_view text) {
    if (text.empty() || !OpenClipboard(window_)) return;
    EmptyClipboard();
    const SIZE_T bytes = (text.size() + 1) * sizeof(wchar_t);
    HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
    if (memory) {
        void* destination = GlobalLock(memory);
        if (destination) {
            std::memcpy(destination, text.data(), text.size() * sizeof(wchar_t));
            static_cast<wchar_t*>(destination)[text.size()] = L'\0';
            GlobalUnlock(memory);
            if (!SetClipboardData(CF_UNICODETEXT, memory)) GlobalFree(memory);
        } else {
            GlobalFree(memory);
        }
    }
    CloseClipboard();
}

void AppHost::retry_translation() {
    if (!last_image_) return;
    BlockEditor::close_active();
    if (pipeline_) pipeline_->cancel_lane(PipelineLane::visual);
    block_translation_request_.reset();
    result_.show_retry_loading(result_appearance());
    current_request_ = 0;
    try {
        if (!last_blocks_.empty()) {
            current_request_ = pipeline_->schedule_translation(
                last_blocks_, PipelineOptions::from_config(config_));
        } else {
            current_request_ = pipeline_->schedule(
                last_image_, PipelineOptions::from_config(config_));
        }
    } catch (const std::exception& error) {
        result_.set_error(L"\u542F\u52A8\u7FFB\u8BD1\u5931\u8D25\uFF1A" +
                          utf8_to_wide(error.what()));
    }
}

void AppHost::open_block_editor() {
    if (!last_result_ || last_result_->blocks.empty() || !result_.handle()) return;
    if (block_translation_request_) {
        notify(L"划词截屏翻译", L"文本块仍在重译，请稍后再打开校对窗口");
        return;
    }
    result_.set_editing(true);
    try {
        const bool opened = BlockEditor::show(instance_, result_.handle(), *last_result_,
            config_.string(L"lang.zh_target", L"en"),
            [this](BlockEditor& editor, BlockEditor::Change change) {
                request_block_translation(editor, change.index, std::move(change.block));
            },
            [this](BlockEditor& editor) { cancel_block_translation(editor); },
            [this](std::optional<std::vector<BlockEditor::Change>> edited) {
                if (!edited) {
                    if (!block_translation_request_) result_.set_editing(false);
                    return;
                }
                std::vector<std::size_t> indices;
                std::vector<TextBlock> blocks;
                indices.reserve(edited->size());
                blocks.reserve(edited->size());
                for (auto& change : *edited) {
                    indices.push_back(change.index);
                    blocks.push_back(std::move(change.block));
                }
                if (blocks.empty() ||
                    !schedule_block_translations(std::move(indices),
                                                 std::move(blocks), nullptr)) {
                    result_.set_editing(false);
                }
            },
            config_.string(L"appearance.accent", L"#28C76F"));
        if (!opened) {
            result_.set_editing(false);
        }
    } catch (const std::exception& error) {
        result_.set_editing(false);
        notify(L"划词截屏翻译", L"无法打开校对窗口：" + utf8_to_wide(error.what()));
    }
}

void AppHost::request_block_translation(BlockEditor& editor, std::size_t index,
                                        TextBlock block) {
    if (!last_result_ || index >= last_result_->blocks.size() || !result_.handle()) {
        editor.set_error(index, L"当前译文已经失效，请重新框选");
        return;
    }
    schedule_block_translations({index}, {std::move(block)}, &editor);
}

void AppHost::cancel_block_translation(BlockEditor& editor) noexcept {
    if (!block_translation_request_ || block_translation_request_->editor != &editor) return;
    // Closing the non-modal editor detaches its UI, but the submitted block
    // remains valid and should still update the result when it completes.
    block_translation_request_->editor = nullptr;
}

bool AppHost::schedule_block_translations(std::vector<std::size_t> indices,
                                          std::vector<TextBlock> blocks,
                                          BlockEditor* editor) {
    if (!pipeline_ || indices.empty() || indices.size() != blocks.size()) return false;
    for (const auto index : indices) {
        if (!last_result_ || index >= last_result_->blocks.size()) {
            if (editor) editor->set_error(index, L"当前译文已经失效，请重新框选");
            return false;
        }
    }

    std::uint64_t request_id{};
    try {
        current_request_ = 0;
        request_id = pipeline_->schedule_translation(
            std::move(blocks), PipelineOptions::from_config(config_));
    } catch (const std::exception& error) {
        auto replaced = std::move(block_translation_request_);
        block_translation_request_.reset();
        pipeline_->cancel_lane(PipelineLane::visual);
        if (replaced && replaced->editor) {
            for (const auto index : replaced->indices) {
                replaced->editor->set_error(
                    index, L"已由新的重译请求替换，请重试");
            }
        }
        const auto message = L"启动单块重译失败：" + utf8_to_wide(error.what());
        if (editor) {
            for (const auto index : indices) editor->set_error(index, message);
        } else {
            notify(L"划词截屏翻译 · 单块重译失败", message);
        }
        return false;
    }

    auto replaced = std::move(block_translation_request_);
    block_translation_request_ = BlockTranslationRequest{
        request_id, std::move(indices), editor,
    };
    if (replaced && replaced->editor) {
        for (const auto index : replaced->indices) {
            replaced->editor->set_error(index, L"已由新的重译请求替换，请重试");
        }
    }
    return true;
}

void AppHost::process_block_translation(PipelineCompletion completion) {
    if (!block_translation_request_ ||
        completion.request_id != block_translation_request_->request_id) {
        return;
    }
    auto request = std::move(*block_translation_request_);
    block_translation_request_.reset();

    const auto fail = [&](std::wstring message) {
        if (request.editor) {
            for (const auto index : request.indices) {
                request.editor->set_error(index, message);
            }
        } else {
            result_.set_editing(false);
            notify(L"划词截屏翻译 · 单块重译失败", message);
        }
    };

    if (!completion.result) {
        fail(completion.error.empty() ? L"单块重译失败" : std::move(completion.error));
        return;
    }
    if (!last_result_ || completion.result->blocks.size() != request.indices.size() ||
        last_result_->blocks.size() != last_blocks_.size()) {
        fail(L"单块重译结果与当前文本块不一致，请重新打开校对窗口");
        return;
    }
    for (const auto index : request.indices) {
        if (index >= last_result_->blocks.size()) {
            fail(L"一个文本块已经失效，请重新打开校对窗口");
            return;
        }
    }

    for (std::size_t offset = 0; offset < request.indices.size(); ++offset) {
        const auto index = request.indices[offset];
        const auto& translated = completion.result->blocks[offset];
        last_result_->blocks[index] = translated;
        last_blocks_[index] = translated.block;
        if (request.editor) request.editor->set_translation(index, translated);
    }
    refresh_cached_result();
    if (!request.editor) result_.set_editing(false);
}

void AppHost::refresh_cached_result() {
    if (!last_result_) return;
    last_result_->plain_text.clear();
    for (const auto& block : last_result_->blocks) {
        if (block.translated.empty()) continue;
        if (!last_result_->plain_text.empty()) last_result_->plain_text.push_back(L'\n');
        last_result_->plain_text += block.translated;
    }
    result_.set_result(*last_result_);
    if (config_.boolean(L"appearance.auto_copy", true) &&
        !last_result_->plain_text.empty()) {
        copy_to_clipboard(last_result_->plain_text);
    }
}

void AppHost::request_recapture(const RECT& rect) {
    BlockEditor::close_active();
    if (pipeline_) pipeline_->cancel_lane(PipelineLane::visual);
    current_request_ = 0;
    block_translation_request_.reset();
    pending_recapture_ = rect;
    KillTimer(window_, recapture_timer);
    // Match the Python client: let the fully transparent result window reach the
    // compositor before capturing the newly resized region.
    SetTimer(window_, recapture_timer, 80, nullptr);
}

void AppHost::recapture_now() {
    if (!pending_recapture_) return;
    const RECT rect = *pending_recapture_;
    pending_recapture_.reset();
    try {
        auto image = std::make_shared<PixelBuffer>(capture_rect(rect));
        last_blocks_.clear();
        start_pipeline(image, rect);
    } catch (const std::exception& error) {
        result_.set_error(L"\u91CD\u65B0\u622A\u53D6\u5931\u8D25\uFF1A" +
                          utf8_to_wide(error.what()));
    }
}

ResultAppearance AppHost::result_appearance() const {
    ResultAppearance value;
    value.font_family = config_.string(L"appearance.font_family", L"Microsoft YaHei UI");
    value.minimum_font_pixels = config_.integer(L"appearance.min_font_px", 9);
    value.accent = parse_rgb_color(config_.string(L"appearance.accent", L"#28C76F"));
    value.close_mode = config_.string(L"appearance.close_mode", L"click");
    value.timeout_ms = config_.integer(L"appearance.timeout_ms", 5000);
    return value;
}

void AppHost::open_settings() {
    try {
        settings_.show(window_);
        refresh_tray_tooltip();
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"无法打开设置：" + utf8_to_wide(error.what()));
    }
}

void AppHost::open_translation_settings() {
    try {
        settings_.show_translation(window_);
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"无法打开翻译设置：" + utf8_to_wide(error.what()));
    }
}

void AppHost::open_updates() {
    try {
        updates_.show(window_);
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"无法检查更新：" + utf8_to_wide(error.what()));
    }
}

void AppHost::start_silent_update_check() {
    if (silent_update_thread_.joinable() || shutting_down_) return;
    auto manifest = config_.string(L"updates.manifest_url");
    const auto repository = config_.string(L"updates.repository_url");
    const auto channel = config_.string(L"updates.channel", L"stable");
    if (manifest.empty() || repository.empty()) return;
    if (update_self_test_mode_) {
        // Exercise the complete worker/message/shutdown path without making a
        // smoke test depend on GitHub or the machine's proxy configuration.
        manifest = L"https://github.com/screentrans-self-test/invalid/"
                   L"releases/latest/download/update-manifest.json";
    }
    const HWND dispatcher = window_;
    silent_update_thread_ = std::jthread(
        [this, dispatcher, manifest, repository, channel](std::stop_token stop) {
            try {
                WinrtApartment apartment(winrt::apartment_type::multi_threaded);
                std::optional<UpdateInfo> result;
                try {
                    result = check_for_update(manifest, repository, channel, stop);
                } catch (...) {
                    result.reset();
                }
                {
                    std::lock_guard lock(silent_update_mutex_);
                    silent_update_result_ = std::move(result);
                }
                PostMessageW(dispatcher, silent_update_completed_message, 0, 0);
            } catch (...) {
                // No exception may escape a std::jthread entry point: doing so
                // invokes std::terminate and aborts the whole tray process.
                PostMessageW(dispatcher, silent_update_completed_message, 0, 0);
            }
        });
}

std::optional<UpdateInfo> AppHost::process_silent_update_check() {
    if (silent_update_thread_.joinable()) silent_update_thread_.join();
    std::lock_guard lock(silent_update_mutex_);
    return std::move(silent_update_result_);
}

void AppHost::handle_update_available(const UpdateInfo& update) {
    available_update_version_ = update.version;
    notify(L"划词截屏翻译 · 有新版本",
           L"ScreenTranslate " + update.version +
           L" 已发布，可从检查更新窗口直接下载并安装。");
}

void AppHost::restart() {
    try {
        const auto executable = executable_path();
        std::wstring command_line = L"\"" + executable.wstring() + L"\" --restart-wait-pid " +
                                    std::to_wstring(GetCurrentProcessId());
        STARTUPINFOW startup{};
        startup.cb = sizeof(startup);
        PROCESS_INFORMATION process{};
        if (!CreateProcessW(executable.c_str(), command_line.data(), nullptr, nullptr, FALSE, 0,
                            nullptr, nullptr, &startup, &process)) {
            throw_last_error("start replacement process");
        }
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
        quit();
    } catch (const std::exception& error) {
        notify(L"划词截屏翻译", L"重启失败：" + utf8_to_wide(error.what()));
    }
}

void AppHost::quit() {
    if (shutting_down_) return;
    shutting_down_ = true;
    PostMessageW(window_, WM_CLOSE, 0, 0);
}

LRESULT CALLBACK AppHost::window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    auto* self = reinterpret_cast<AppHost*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    if (message == WM_NCCREATE) {
        const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
        self = static_cast<AppHost*>(create->lpCreateParams);
        self->window_ = window;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    }
    return self ? self->handle_message(message, wparam, lparam)
                : DefWindowProcW(window, message, wparam, lparam);
}

LRESULT AppHost::handle_message(UINT message, WPARAM wparam, LPARAM lparam) {
    if (taskbar_created_message_ && message == taskbar_created_message_) {
        tray_.hWnd = nullptr;
        if (icon_) {
            DestroyIcon(icon_);
            icon_ = nullptr;
        }
        tray_accent_.clear();
        try {
            add_tray_icon();
        } catch (...) {
            // Explorer can broadcast TaskbarCreated before its notification area is ready.
        }
        return 0;
    }
    switch (message) {
    case initialize_message:
        if (self_test_mode_) return 0;
        try {
            add_tray_icon();
            apply_hotkeys(false);
            notify(L"划词截屏翻译",
                   L"已启动，按 " + config_.string(L"hotkey", L"Ctrl+Alt+Q") +
                   L" 开始框选翻译");
            SetTimer(window_, update_timer, 1500, nullptr);
        } catch (const std::exception& error) {
            const auto message_text = L"ScreenTranslate 无法初始化通知区域图标。\n\n" +
                                      utf8_to_wide(error.what());
            MessageBoxW(nullptr, message_text.c_str(), L"ScreenTranslate",
                        MB_OK | MB_ICONERROR);
            quit();
        }
        return 0;
    case activate_message: toggle_result(); return 0;
    case WM_HOTKEY:
        if (wparam == hotkey_capture) start_capture();
        else if (wparam == hotkey_toggle) toggle_result();
        else if (wparam == hotkey_text_translate) toggle_quick_translation();
        return 0;
    case tray_message: {
        const UINT event = LOWORD(lparam);
        if (event == WM_LBUTTONUP || event == NIN_SELECT || event == NIN_KEYSELECT) {
            start_capture();
        } else if (event == WM_LBUTTONDBLCLK) {
            open_settings();
        } else if (event == WM_RBUTTONUP || event == WM_CONTEXTMENU) {
            POINT point{}; GetCursorPos(&point); show_tray_menu(point);
        }
        return 0;
    }
    case WM_MEASUREITEM:
        if (wparam == 0 && lparam && measure_tray_menu_item(
                *reinterpret_cast<MEASUREITEMSTRUCT*>(lparam))) {
            return TRUE;
        }
        break;
    case WM_DRAWITEM:
        if (wparam == 0 && lparam && draw_tray_menu_item(
                *reinterpret_cast<const DRAWITEMSTRUCT*>(lparam))) {
            return TRUE;
        }
        break;
    case WM_COMMAND:
        switch (LOWORD(wparam)) {
        case command_capture: start_capture(); return 0;
        case command_text_translate: open_text_translation(); return 0;
        case command_restore:
            if (last_image_ && !result_.visible()) result_.restore();
            return 0;
        case command_settings: open_settings(); return 0;
        case command_updates: open_updates(); return 0;
        case command_restart: restart(); return 0;
        case command_exit: quit(); return 0;
        default: break;
        }
        break;
    case WM_TIMER:
        if (wparam == capture_timer) {
            KillTimer(window_, capture_timer); begin_capture_now(); return 0;
        }
        if (wparam == recapture_timer) {
            KillTimer(window_, recapture_timer); recapture_now(); return 0;
        }
        if (wparam == update_timer) {
            KillTimer(window_, update_timer); start_silent_update_check(); return 0;
        }
        if (wparam == text_translate_timer) {
            KillTimer(window_, text_translate_timer);
            schedule_text_translation(true);
            return 0;
        }
        break;
    case pipeline_completed_message: process_pipeline_completions(); return 0;
    case silent_update_completed_message: {
        auto update = process_silent_update_check();
        if (update) handle_update_available(*update);
        if (update_self_test_mode_) PostMessageW(window_, WM_CLOSE, 0, 0);
        return 0;
    }
    case WM_CLOSE:
        shutting_down_ = true;
        DestroyWindow(window_);
        return 0;
    case WM_DESTROY:
        remove_tray_icon();
        UnregisterHotKey(window_, hotkey_capture);
        UnregisterHotKey(window_, hotkey_toggle);
        UnregisterHotKey(window_, hotkey_text_translate);
        PostQuitMessage(0);
        return 0;
    case WM_NCDESTROY:
        SetWindowLongPtrW(window_, GWLP_USERDATA, 0);
        window_ = nullptr;
        return 0;
    default: break;
    }
    return DefWindowProcW(window_, message, wparam, lparam);
}

int AppHost::run(bool self_test, bool update_self_test) {
    self_test_mode_ = self_test || update_self_test;
    update_self_test_mode_ = update_self_test;
    if (update_self_test) {
        start_silent_update_check();
    }
    if (self_test) {
        PipelineController::self_test();
        if (text_session_) text_session_->self_test();
        tray_menu_layout_self_test();
        HICON tinted_icon = load_accent_icon(instance_, RGB(236, 76, 92));
        const bool tint_ok = icon_contains_red_accent(tinted_icon);
        if (tinted_icon) DestroyIcon(tinted_icon);
        if (!tint_ok) throw AppError("tray icon accent self-test failed");
        BlockEditor::self_test(instance_, window_);
        settings_.self_test(window_);
        updates_.self_test(window_);
        if (quick_window_) quick_window_->self_test(window_);
        if (!PostMessageW(window_, WM_CLOSE, 0, 0)) {
            throw_last_error("queue self-test shutdown");
        }
    }
    MSG message{};
    BOOL result = 0;
    while ((result = GetMessageW(&message, nullptr, 0, 0)) > 0) {
        if (BlockEditor::preprocess_active_message(message) ||
            settings_.preprocess_message(message) ||
            updates_.preprocess_message(message) ||
            (quick_window_ && quick_window_->preprocess_message(message))) {
            continue;
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    if (result == -1) throw_last_error("message loop failed");
    return static_cast<int>(message.wParam);
}

}  // namespace screentrans
