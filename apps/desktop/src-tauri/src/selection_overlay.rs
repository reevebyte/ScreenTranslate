//! Native Win32 fullscreen selection overlay (Snipaste/PixPin style).
//!
//! Renders captured monitor bitmaps directly via a double-buffered GDI DIBSection
//! on opaque topmost `WS_POPUP` windows, completely bypassing WebView2 /
//! DirectComposition cold-start and MPO desktop twitching on Windows.

use screentrans_core::Rect;

/// Selection box in window-local physical pixels (`x0 <= x1`, `y0 <= y1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl PixelRect {
    fn from_points(ax: i32, ay: i32, bx: i32, by: i32, max_w: i32, max_h: i32) -> Self {
        let ax = ax.clamp(0, max_w);
        let ay = ay.clamp(0, max_h);
        let bx = bx.clamp(0, max_w);
        let by = by.clamp(0, max_h);
        Self {
            x0: ax.min(bx),
            y0: ay.min(by),
            x1: ax.max(bx),
            y1: ay.max(by),
        }
    }

    fn width(self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    fn height(self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    fn meets_minimum(self, scale: f64) -> bool {
        let min_px = (4.0 * scale.max(1.0)).round().max(4.0) as i32;
        self.width() >= min_px && self.height() >= min_px
    }

    fn to_image_rect(self, win_w: i32, win_h: i32, img_w: u32, img_h: u32) -> Rect {
        let sx = f64::from(img_w) / f64::from(win_w.max(1));
        let sy = f64::from(img_h) / f64::from(win_h.max(1));
        Rect {
            x: f64::from(self.x0) * sx,
            y: f64::from(self.y0) * sy,
            width: f64::from(self.width()) * sx,
            height: f64::from(self.height()) * sy,
        }
    }
}

fn checked_bgra_len(width: i32, height: i32) -> Option<usize> {
    const MAX_OVERLAY_BYTES: usize = 256 * 1024 * 1024;
    let w = usize::try_from(width).ok().filter(|&v| v > 0)?;
    let h = usize::try_from(height).ok().filter(|&v| v > 0)?;
    w.checked_mul(h)?
        .checked_mul(4)
        .filter(|&bytes| bytes <= MAX_OVERLAY_BYTES)
}

/// Blits `src` (`src_w × src_h` BGRA) into `dst` (`dst_w × dst_h` BGRA),
/// zero-filling any padded region if dimensions differ.
fn copy_bgra_surface(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    src: &[u8],
    src_w: usize,
    src_h: usize,
) {
    let Some(dst_len) = dst_w.checked_mul(dst_h).and_then(|n| n.checked_mul(4)) else {
        return;
    };
    if dst.len() < dst_len {
        return;
    }
    if dst_w == src_w && dst_h == src_h && src.len() >= dst_len {
        dst[..dst_len].copy_from_slice(&src[..dst_len]);
        return;
    }
    dst[..dst_len].fill(0);
    let copy_w = dst_w.min(src_w);
    let copy_h = dst_h.min(src_h);
    let row_bytes = copy_w * 4;
    let src_stride = src_w * 4;
    let dst_stride = dst_w * 4;
    for y in 0..copy_h {
        let s0 = y * src_stride;
        let d0 = y * dst_stride;
        if s0 + row_bytes <= src.len() && d0 + row_bytes <= dst_len {
            dst[d0..d0 + row_bytes].copy_from_slice(&src[s0..s0 + row_bytes]);
        }
    }
}

#[derive(Clone, Copy)]
struct RoundedBoxStyle {
    radius: f64,
    fill: (u8, u8, u8, f64),
    border: Option<(u8, u8, u8, f64, f64)>,
    shadow: Option<(f64, f64, f64)>,
}

struct Canvas<'a> {
    buf: &'a mut [u8],
    width: i32,
    height: i32,
}

impl<'a> Canvas<'a> {
    fn new(buf: &'a mut [u8], width: i32, height: i32) -> Self {
        Self { buf, width, height }
    }

    #[inline]
    fn blend_pixel(&mut self, x: i32, y: i32, rgb: (u8, u8, u8), alpha: f64) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height || alpha <= 0.0 {
            return;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        if idx + 3 >= self.buf.len() {
            return;
        }
        let a = alpha.clamp(0.0, 1.0);
        if a >= 0.999 {
            self.buf[idx] = rgb.2;
            self.buf[idx + 1] = rgb.1;
            self.buf[idx + 2] = rgb.0;
            self.buf[idx + 3] = 255;
            return;
        }
        let inv = 1.0 - a;
        self.buf[idx] = (f64::from(self.buf[idx]) * inv + f64::from(rgb.2) * a).round() as u8;
        self.buf[idx + 1] =
            (f64::from(self.buf[idx + 1]) * inv + f64::from(rgb.1) * a).round() as u8;
        self.buf[idx + 2] =
            (f64::from(self.buf[idx + 2]) * inv + f64::from(rgb.0) * a).round() as u8;
        self.buf[idx + 3] = 255;
    }

    fn fill_solid_rect(&mut self, rect: PixelRect, rgb: (u8, u8, u8)) {
        if self.width <= 0 || self.height <= 0 {
            return;
        }
        let left = rect.x0.clamp(0, self.width) as usize;
        let right = rect.x1.clamp(0, self.width) as usize;
        let top = rect.y0.clamp(0, self.height) as usize;
        let bottom = rect.y1.clamp(0, self.height) as usize;
        let stride = (self.width as usize).saturating_mul(4);
        for y in top..bottom {
            let row = y.saturating_mul(stride);
            for x in left..right {
                let idx = row.saturating_add(x.saturating_mul(4));
                if idx + 3 >= self.buf.len() {
                    break;
                }
                self.buf[idx] = rgb.2;
                self.buf[idx + 1] = rgb.1;
                self.buf[idx + 2] = rgb.0;
                self.buf[idx + 3] = 255;
            }
        }
    }

    /// Anti-aliased rounded rectangle with optional border and soft drop shadow,
    /// matching the visual language of `CaptureOverlay.svelte` / `DESIGN.md`.
    fn draw_rounded_box(&mut self, bounds: (f64, f64, f64, f64), style: RoundedBoxStyle) {
        let (left, top, right, bottom) = bounds;
        let cx = (left + right) * 0.5;
        let cy = (top + bottom) * 0.5;
        let half_w = ((right - left) * 0.5).max(0.5);
        let half_h = ((bottom - top) * 0.5).max(0.5);
        let r = style.radius.min(half_w).min(half_h).max(0.5);
        let pad = style
            .shadow
            .map_or(2.0, |(_, blur, dy)| blur + dy.abs() + 2.0);
        let min_x = (left - pad).floor().max(0.0) as i32;
        let max_x = (right + pad).ceil().min(f64::from(self.width)) as i32;
        let min_y = (top - pad).floor().max(0.0) as i32;
        let max_y = (bottom + pad).ceil().min(f64::from(self.height)) as i32;

        for y in min_y..max_y {
            let fy = f64::from(y) + 0.5;
            for x in min_x..max_x {
                let fx = f64::from(x) + 0.5;
                let qx = (fx - cx).abs() - (half_w - r);
                let qy = (fy - cy).abs() - (half_h - r);
                let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let fill_cov = (0.5 - d).clamp(0.0, 1.0);
                if let Some((shadow_alpha, blur, dy)) = style.shadow {
                    let outside = 1.0 - fill_cov;
                    if outside > 0.0 && blur > 0.0 {
                        let sqy = (fy - (cy + dy)).abs() - (half_h - r);
                        let d_shadow = qx.max(0.0).hypot(sqy.max(0.0)) + qx.max(sqy).min(0.0) - r;
                        if d_shadow < blur {
                            let inset = (blur * 0.4).max(1.0);
                            let u = ((d_shadow + inset) / (blur + inset)).clamp(0.0, 1.0);
                            let s = 1.0 - u;
                            let weight = s * s * (3.0 - 2.0 * s);
                            self.blend_pixel(x, y, (0, 0, 0), shadow_alpha * weight * outside);
                        }
                    }
                }
                if fill_cov > 0.0 {
                    self.blend_pixel(
                        x,
                        y,
                        (style.fill.0, style.fill.1, style.fill.2),
                        style.fill.3 * fill_cov,
                    );
                }
                if let Some((br, bg, bb, ba, stroke_w)) = style.border {
                    let d_border = (d + 0.5 * stroke_w).abs() - 0.5 * stroke_w;
                    let border_cov = (0.5 - d_border).clamp(0.0, 1.0);
                    if border_cov > 0.0 {
                        self.blend_pixel(x, y, (br, bg, bb), ba * border_cov);
                    }
                }
            }
        }
    }

    /// Draws the 2px `#168bff` selection outline and 8 anti-aliased 6px circular
    /// white anchors with `#168bff` 1px borders (`DESIGN.md` screenshot frame spec).
    fn draw_selection_frame(&mut self, sel: PixelRect, scale: f64) {
        const BLUE: (u8, u8, u8) = (0x16, 0x8b, 0xff);

        let stroke = (2.0 * scale).round().max(2.0) as i32;
        self.fill_solid_rect(
            PixelRect {
                x0: sel.x0 - stroke,
                y0: sel.y0 - stroke,
                x1: sel.x1 + stroke,
                y1: sel.y0,
            },
            BLUE,
        );
        self.fill_solid_rect(
            PixelRect {
                x0: sel.x0 - stroke,
                y0: sel.y1,
                x1: sel.x1 + stroke,
                y1: sel.y1 + stroke,
            },
            BLUE,
        );
        self.fill_solid_rect(
            PixelRect {
                x0: sel.x0 - stroke,
                y0: sel.y0,
                x1: sel.x0,
                y1: sel.y1,
            },
            BLUE,
        );
        self.fill_solid_rect(
            PixelRect {
                x0: sel.x1,
                y0: sel.y0,
                x1: sel.x1 + stroke,
                y1: sel.y1,
            },
            BLUE,
        );

        let radius = (3.0 * scale).max(3.0);
        let ring = scale.max(1.0);
        let outer_r = radius + ring;
        let mid_x = f64::from(sel.x0 + sel.x1) * 0.5;
        let mid_y = f64::from(sel.y0 + sel.y1) * 0.5;
        let anchors = [
            (f64::from(sel.x0), f64::from(sel.y0)),
            (mid_x, f64::from(sel.y0)),
            (f64::from(sel.x1), f64::from(sel.y0)),
            (f64::from(sel.x1), mid_y),
            (f64::from(sel.x1), f64::from(sel.y1)),
            (mid_x, f64::from(sel.y1)),
            (f64::from(sel.x0), f64::from(sel.y1)),
            (f64::from(sel.x0), mid_y),
        ];

        for (cx, cy) in anchors {
            let min_x = (cx - outer_r - 1.0).floor().max(0.0) as i32;
            let max_x = (cx + outer_r + 1.0).ceil().min(f64::from(self.width)) as i32;
            let min_y = (cy - outer_r - 1.0).floor().max(0.0) as i32;
            let max_y = (cy + outer_r + 1.0).ceil().min(f64::from(self.height)) as i32;
            for y in min_y..max_y {
                let fy = f64::from(y) + 0.5;
                for x in min_x..max_x {
                    let fx = f64::from(x) + 0.5;
                    let dist = (fx - cx).hypot(fy - cy);
                    let ring_cov = (outer_r + 0.5 - dist).clamp(0.0, 1.0);
                    if ring_cov > 0.0 {
                        self.blend_pixel(x, y, BLUE, ring_cov);
                    }
                    let fill_cov = (radius + 0.5 - dist).clamp(0.0, 1.0);
                    if fill_cov > 0.0 {
                        self.blend_pixel(x, y, (255, 255, 255), fill_cov);
                    }
                }
            }
        }
    }

    /// Draws a crisp ScanLine-style corner viewfinder glyph inside the guide pill.
    fn draw_scan_icon(&mut self, cx: f64, cy: f64, scale: f64, rgb: (u8, u8, u8)) {
        let half = 5.5 * scale;
        let arm = 2.6 * scale;
        let stroke = (0.75 * scale).max(0.75);
        let segments = [
            (cx - half, cy - half, cx - half + arm, cy - half),
            (cx - half, cy - half, cx - half, cy - half + arm),
            (cx + half - arm, cy - half, cx + half, cy - half),
            (cx + half, cy - half, cx + half, cy - half + arm),
            (cx - half, cy + half, cx - half + arm, cy + half),
            (cx - half, cy + half - arm, cx - half, cy + half),
            (cx + half - arm, cy + half, cx + half, cy + half),
            (cx + half, cy + half - arm, cx + half, cy + half),
            (cx - half + 0.8 * scale, cy, cx + half - 0.8 * scale, cy),
        ];
        let min_x = (cx - half - 2.0).floor().max(0.0) as i32;
        let max_x = (cx + half + 2.0).ceil().min(f64::from(self.width)) as i32;
        let min_y = (cy - half - 2.0).floor().max(0.0) as i32;
        let max_y = (cy + half + 2.0).ceil().min(f64::from(self.height)) as i32;
        for y in min_y..max_y {
            let fy = f64::from(y) + 0.5;
            for x in min_x..max_x {
                let fx = f64::from(x) + 0.5;
                let mut cov = 0.0_f64;
                for &(ax, ay, bx, by) in &segments {
                    let vx = bx - ax;
                    let vy = by - ay;
                    let len2 = vx * vx + vy * vy;
                    let t = if len2 > 0.0 {
                        (((fx - ax) * vx + (fy - ay) * vy) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let d = (fx - ax - t * vx).hypot(fy - ay - t * vy);
                    cov = cov.max((stroke + 0.5 - d).clamp(0.0, 1.0));
                }
                if cov > 0.0 {
                    self.blend_pixel(x, y, rgb, cov);
                }
            }
        }
    }
}

/// Converts an `RgbaImage` pixel slice into a top-down 32-bit BGRA buffer for
/// Win32 `CreateDIBSection` (`biHeight = -height`).
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = vec![0u8; rgba.len()];
    for (src, dst) in rgba.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 255;
    }
    bgra
}

/// Computes the screen-clamped position of the selection dimensions badge.
fn dimensions_badge_rect(
    sel: PixelRect,
    win_w: i32,
    win_h: i32,
    badge_w: i32,
    badge_h: i32,
    scale: f64,
) -> (i32, i32, i32, i32) {
    let gap = (6.0 * scale).round().max(4.0) as i32;
    let mut left = sel.x0;
    if left + badge_w > win_w - gap {
        left = (win_w - gap - badge_w).max(gap);
    }
    let top = if sel.y0 - gap - badge_h >= gap {
        sel.y0 - gap - badge_h
    } else {
        (sel.y0 + gap).min((win_h - gap - badge_h).max(0))
    };
    (left, top, left + badge_w, top + badge_h)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::{ui_locale::UiLocale, windows::window_error};
    use image::RgbaImage;
    use screentrans_core::{AppError, Result};
    use std::{
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    use tauri::AppHandle;
    use tokio_util::sync::CancellationToken;
    use windows::{
        Win32::{
            Foundation::{COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
            Graphics::{
                Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute},
                Gdi::{
                    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY,
                    CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW,
                    DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT,
                    DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
                    DrawTextW, EndPaint, FW_MEDIUM, FW_NORMAL, FW_SEMIBOLD, GdiFlush, GetDC,
                    GetStockObject, HBITMAP, HDC, HFONT, HGDIOBJ, InvalidateRect,
                    OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, SRCCOPY, SelectObject, SetBkMode,
                    SetTextColor, TRANSPARENT, UpdateWindow,
                },
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                Input::KeyboardAndMouse::{
                    GetCapture, ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE,
                },
                WindowsAndMessaging::{
                    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                    GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, HMENU, HWND_TOPMOST, IDC_CROSS,
                    IsWindow, LoadCursorW, RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                    SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
                    WM_CAPTURECHANGED, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
                    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONDOWN,
                    WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
                    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
                },
            },
        },
        core::w,
    };

    pub struct MonitorOverlaySpec {
        pub label: String,
        pub x: i32,
        pub y: i32,
        pub width: i32,
        pub height: i32,
        pub scale: f64,
        pub image: Arc<RgbaImage>,
    }

    struct GuidePalette {
        pill_fill: (u8, u8, u8, f64),
        pill_border: (u8, u8, u8, f64, f64),
        pill_shadow: (f64, f64, f64),
        icon_rgb: (u8, u8, u8),
        main_bgr: u32,
        kbd_fill: (u8, u8, u8, f64),
        kbd_border: (u8, u8, u8, f64, f64),
        kbd_bgr: u32,
        cancel_bgr: u32,
        div_rgba: (u8, u8, u8, f64),
    }

    impl GuidePalette {
        fn for_theme(dark: bool, scale: f64) -> Self {
            if dark {
                Self {
                    pill_fill: (32, 28, 29, 0.92),
                    pill_border: (255, 255, 255, 0.14, scale.max(1.0)),
                    pill_shadow: (0.34, 14.0 * scale, 4.0 * scale),
                    icon_rgb: (0xa6, 0x9e, 0xaf),
                    main_bgr: 0x00f0edf3,
                    kbd_fill: (255, 255, 255, 0.12),
                    kbd_border: (255, 255, 255, 0.20, scale.max(1.0)),
                    kbd_bgr: 0x00ffffff,
                    cancel_bgr: 0x00af9ea6,
                    div_rgba: (255, 255, 255, 0.16),
                }
            } else {
                Self {
                    pill_fill: (255, 255, 255, 0.92),
                    pill_border: (0, 0, 0, 0.10, scale.max(1.0)),
                    pill_shadow: (0.14, 14.0 * scale, 4.0 * scale),
                    icon_rgb: (0x52, 0x52, 0x5b),
                    main_bgr: 0x001b1818,
                    kbd_fill: (0, 0, 0, 0.06),
                    kbd_border: (0, 0, 0, 0.10, scale.max(1.0)),
                    kbd_bgr: 0x001b1818,
                    cancel_bgr: 0x007a7171,
                    div_rgba: (0, 0, 0, 0.12),
                }
            }
        }
    }

    #[derive(Default)]
    struct OverlayRegistry {
        generation: u64,
        handles: Vec<isize>,
    }

    static REGISTRY: OnceLock<Mutex<OverlayRegistry>> = OnceLock::new();

    fn registry() -> &'static Mutex<OverlayRegistry> {
        REGISTRY.get_or_init(|| Mutex::new(OverlayRegistry::default()))
    }

    pub fn is_active() -> bool {
        let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
        guard
            .handles
            .iter()
            .any(|&raw| unsafe { IsWindow(HWND(raw as *mut _)).as_bool() })
    }

    /// Schedules asynchronous destruction of any active native overlays.
    pub fn close(app: &AppHandle) {
        let handles = {
            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
            reg.generation = reg.generation.wrapping_add(1);
            std::mem::take(&mut reg.handles)
        };
        if handles.is_empty() {
            return;
        }
        let _ = app.run_on_main_thread(move || destroy_handles(&handles));
    }

    /// Destroys any active native overlays on the UI thread and awaits completion
    /// so a subsequent `BitBlt` screen capture cannot observe stale overlays.
    /// Returns `true` if at least one overlay was closed.
    pub async fn close_and_wait(app: &AppHandle) -> bool {
        let handles = {
            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
            reg.generation = reg.generation.wrapping_add(1);
            std::mem::take(&mut reg.handles)
        };
        if handles.is_empty() {
            return false;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        if app
            .run_on_main_thread(move || {
                destroy_handles(&handles);
                let _ = tx.send(());
            })
            .is_ok()
        {
            let _ = tokio::time::timeout(Duration::from_secs(2), rx).await;
        }
        true
    }

    fn destroy_handles(handles: &[isize]) {
        for &raw in handles {
            unsafe {
                let hwnd = HWND(raw as *mut _);
                if IsWindow(hwnd).as_bool() {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }

    struct OverlayWindow {
        app: AppHandle,
        label: String,
        width: i32,
        height: i32,
        img_width: u32,
        img_height: u32,
        scale: f64,
        dark: bool,
        locale: UiLocale,
        bg_bgra: Vec<u8>,
        back_dc: HDC,
        back_bmp: HBITMAP,
        old_bmp: HGDIOBJ,
        back_bits: *mut u8,
        font_guide: HFONT,
        font_kbd: HFONT,
        font_dim: HFONT,
        anchor: Option<(i32, i32)>,
        selection: Option<PixelRect>,
        submitting: Arc<AtomicBool>,
    }

    impl Drop for OverlayWindow {
        fn drop(&mut self) {
            unsafe {
                for font in [self.font_guide, self.font_kbd, self.font_dim] {
                    if !font.0.is_null() {
                        let _ = DeleteObject(HGDIOBJ(font.0));
                    }
                }
                if !self.back_dc.0.is_null() {
                    if !self.old_bmp.0.is_null() {
                        let _ = SelectObject(self.back_dc, self.old_bmp);
                    }
                    let _ = DeleteDC(self.back_dc);
                }
                if !self.back_bmp.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.back_bmp.0));
                }
            }
        }
    }

    unsafe fn create_ui_font(pixel_height: i32, weight: u32) -> HFONT {
        unsafe {
            CreateFontW(
                -pixel_height.max(10),
                0,
                0,
                0,
                weight as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                DEFAULT_PITCH.0 as u32,
                w!("Microsoft YaHei UI"),
            )
        }
    }

    unsafe fn measure_text(dc: HDC, font: HFONT, text: &str) -> (i32, i32) {
        unsafe {
            let old = SelectObject(
                dc,
                if font.0.is_null() {
                    GetStockObject(DEFAULT_GUI_FONT)
                } else {
                    HGDIOBJ(font.0)
                },
            );
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let mut rc = RECT::default();
            DrawTextW(
                dc,
                &mut wide,
                &mut rc,
                DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
            );
            let _ = SelectObject(dc, old);
            (rc.right - rc.left, rc.bottom - rc.top)
        }
    }

    unsafe fn draw_text_centered(dc: HDC, font: HFONT, color_bgr: u32, text: &str, mut rc: RECT) {
        unsafe {
            let old = SelectObject(
                dc,
                if font.0.is_null() {
                    GetStockObject(DEFAULT_GUI_FONT)
                } else {
                    HGDIOBJ(font.0)
                },
            );
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(color_bgr));
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            DrawTextW(
                dc,
                &mut wide,
                &mut rc,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            let _ = SelectObject(dc, old);
        }
    }

    impl OverlayWindow {
        unsafe fn create_backbuffer(
            width: i32,
            height: i32,
        ) -> Result<(HDC, HBITMAP, HGDIOBJ, *mut u8)> {
            if checked_bgra_len(width, height).is_none() {
                return Err(window_error());
            }
            unsafe {
                let screen_dc = GetDC(HWND::default());
                let back_dc = CreateCompatibleDC(screen_dc);
                let _ = ReleaseDC(HWND::default(), screen_dc);
                if back_dc.0.is_null() {
                    return Err(window_error());
                }
                let mut info = BITMAPINFO::default();
                info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = width;
                info.bmiHeader.biHeight = -height;
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB.0;
                let mut bits_ptr = std::ptr::null_mut();
                let back_bmp = match CreateDIBSection(
                    back_dc,
                    &info,
                    DIB_RGB_COLORS,
                    &mut bits_ptr,
                    HANDLE::default(),
                    0,
                ) {
                    Ok(bmp) if !bits_ptr.is_null() => bmp,
                    _ => {
                        let _ = DeleteDC(back_dc);
                        return Err(window_error());
                    }
                };
                let old_bmp = SelectObject(back_dc, HGDIOBJ(back_bmp.0));
                Ok((back_dc, back_bmp, old_bmp, bits_ptr as *mut u8))
            }
        }

        unsafe fn paint(&self, paint_dc: HDC) {
            let Some(byte_len) = checked_bgra_len(self.width, self.height) else {
                return;
            };
            unsafe {
                let _ = GdiFlush();
                let buf = std::slice::from_raw_parts_mut(self.back_bits, byte_len);
                copy_bgra_surface(
                    buf,
                    self.width as usize,
                    self.height as usize,
                    &self.bg_bgra,
                    self.img_width as usize,
                    self.img_height as usize,
                );
                let mut canvas = Canvas::new(buf, self.width, self.height);
                let scale = self.scale;

                // 1. Selection frame & dimensions badge
                let mut dim_draw: Option<(String, RECT)> = None;
                if let Some(sel) = self.selection
                    && sel.width() > 0
                    && sel.height() > 0
                {
                    canvas.draw_selection_frame(sel, scale);
                    let img_rect =
                        sel.to_image_rect(self.width, self.height, self.img_width, self.img_height);
                    let dim_text = format!(
                        "{} × {}",
                        img_rect.width.round() as i32,
                        img_rect.height.round() as i32
                    );
                    let (tw, th) = measure_text(self.back_dc, self.font_dim, &dim_text);
                    let pad_x = (7.0 * scale).round() as i32;
                    let pad_y = (3.0 * scale).round() as i32;
                    let badge_w = tw + pad_x * 2;
                    let badge_h = th + pad_y * 2;
                    let (l, t, r, b) = dimensions_badge_rect(
                        sel,
                        self.width,
                        self.height,
                        badge_w,
                        badge_h,
                        scale,
                    );
                    canvas.draw_rounded_box(
                        (f64::from(l), f64::from(t), f64::from(r), f64::from(b)),
                        RoundedBoxStyle {
                            radius: (4.0 * scale).max(3.0),
                            fill: (0x25, 0x29, 0x25, 0.96),
                            border: None,
                            shadow: None,
                        },
                    );
                    dim_draw = Some((
                        dim_text,
                        RECT {
                            left: l,
                            top: t,
                            right: r,
                            bottom: b,
                        },
                    ));
                }

                // 2. Top guide pill
                let submitting = self.submitting.load(Ordering::Acquire);
                let main_text = if submitting {
                    self.locale.selection_recognizing()
                } else {
                    self.locale.selection_drag()
                };
                let cancel_text = self.locale.selection_cancel();
                let (main_w, main_h) = measure_text(self.back_dc, self.font_guide, main_text);
                let (esc_w, esc_h) = measure_text(self.back_dc, self.font_kbd, "Esc");
                let (cancel_w, _) = measure_text(self.back_dc, self.font_guide, cancel_text);

                let px = |v: f64| (v * scale).round() as i32;
                let pad_l = px(14.0);
                let pad_r = px(16.0);
                let pad_y = px(7.0);
                let icon_w = px(16.0);
                let gap = px(10.0);
                let div_w = (1.0 * scale).round().max(1.0) as i32;
                let kbd_pad_x = px(7.0);
                let kbd_w = esc_w + kbd_pad_x * 2;
                let kbd_h = (esc_h + px(4.0)).max(px(20.0));
                let pill_w = pad_l
                    + icon_w
                    + gap
                    + main_w
                    + gap
                    + div_w
                    + gap
                    + kbd_w
                    + px(8.0)
                    + cancel_w
                    + pad_r;
                let pill_h = (main_h.max(kbd_h) + pad_y * 2).max(px(34.0));
                let pill_l = (self.width - pill_w) / 2;
                let pill_t = px(48.0).min((self.height - pill_h - px(8.0)).max(0));
                let pill_r = pill_l + pill_w;
                let pill_b = pill_t + pill_h;

                let palette = GuidePalette::for_theme(self.dark, scale);
                canvas.draw_rounded_box(
                    (
                        f64::from(pill_l),
                        f64::from(pill_t),
                        f64::from(pill_r),
                        f64::from(pill_b),
                    ),
                    RoundedBoxStyle {
                        radius: f64::from(pill_h) * 0.5,
                        fill: palette.pill_fill,
                        border: Some(palette.pill_border),
                        shadow: Some(palette.pill_shadow),
                    },
                );

                let icon_cx = f64::from(pill_l + pad_l) + f64::from(icon_w) * 0.5;
                let pill_cy = f64::from(pill_t + pill_b) * 0.5;
                canvas.draw_scan_icon(icon_cx, pill_cy, scale, palette.icon_rgb);

                let main_l = pill_l + pad_l + icon_w + gap;
                let div_x = main_l + main_w + gap;
                let div_h = px(14.0);
                let div_t = pill_t + (pill_h - div_h) / 2;
                let div_rgb = (palette.div_rgba.0, palette.div_rgba.1, palette.div_rgba.2);
                for y in div_t..(div_t + div_h) {
                    for dx in 0..div_w {
                        canvas.blend_pixel(div_x + dx, y, div_rgb, palette.div_rgba.3);
                    }
                }

                let kbd_l = div_x + div_w + gap;
                let kbd_t = pill_t + (pill_h - kbd_h) / 2;
                let kbd_r = kbd_l + kbd_w;
                let kbd_b = kbd_t + kbd_h;
                canvas.draw_rounded_box(
                    (
                        f64::from(kbd_l),
                        f64::from(kbd_t),
                        f64::from(kbd_r),
                        f64::from(kbd_b),
                    ),
                    RoundedBoxStyle {
                        radius: f64::from(kbd_h) * 0.5,
                        fill: palette.kbd_fill,
                        border: Some(palette.kbd_border),
                        shadow: None,
                    },
                );

                // NLL ends `canvas`'s mutable borrow of `back_bits` before GDI `DrawTextW`.
                if let Some((dim_text, dim_rc)) = dim_draw {
                    draw_text_centered(self.back_dc, self.font_dim, 0x00ffffff, &dim_text, dim_rc);
                }
                draw_text_centered(
                    self.back_dc,
                    self.font_guide,
                    palette.main_bgr,
                    main_text,
                    RECT {
                        left: main_l,
                        top: pill_t,
                        right: main_l + main_w,
                        bottom: pill_b,
                    },
                );
                draw_text_centered(
                    self.back_dc,
                    self.font_kbd,
                    palette.kbd_bgr,
                    "Esc",
                    RECT {
                        left: kbd_l,
                        top: kbd_t,
                        right: kbd_r,
                        bottom: kbd_b,
                    },
                );
                let cancel_l = kbd_r + px(8.0);
                draw_text_centered(
                    self.back_dc,
                    self.font_guide,
                    palette.cancel_bgr,
                    cancel_text,
                    RECT {
                        left: cancel_l,
                        top: pill_t,
                        right: cancel_l + cancel_w,
                        bottom: pill_b,
                    },
                );

                let _ = BitBlt(
                    paint_dc,
                    0,
                    0,
                    self.width,
                    self.height,
                    self.back_dc,
                    0,
                    0,
                    SRCCOPY,
                );
            }
        }
    }

    fn pointer_pos(lparam: LPARAM) -> (i32, i32) {
        let x = lparam.0 as u16 as i16 as i32;
        let y = (lparam.0 >> 16) as u16 as i16 as i32;
        (x, y)
    }

    fn cancel_async(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            crate::windows::cancel_capture(&app, true);
        });
    }

    unsafe extern "system" fn selection_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: Never hold `&mut OverlayWindow` or `&OverlayWindow` across any
        // Win32 call that can synchronously re-enter `selection_wnd_proc`
        // (`SetFocus`, `SetCapture`, `ReleaseCapture`, `UpdateWindow`, etc.).
        unsafe {
            match msg {
                WM_ERASEBKGND => LRESULT(1),
                WM_SETCURSOR => {
                    if let Ok(cross) = LoadCursorW(HINSTANCE::default(), IDC_CROSS) {
                        let _ = SetCursor(cross);
                        return LRESULT(1);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let paint_dc = BeginPaint(hwnd, &mut ps);
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        (*ptr).paint(paint_dc);
                    }
                    let _ = EndPaint(hwnd, &ps);
                    LRESULT(0)
                }
                WM_LBUTTONDOWN => {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    let can_start = !ptr.is_null() && !(*ptr).submitting.load(Ordering::Acquire);
                    if can_start {
                        let _ = SetFocus(hwnd);
                        if GetCapture() != hwnd {
                            let _ = SetCapture(hwnd);
                        }
                        // Re-read GWLP_USERDATA in case a synchronous message destroyed the window.
                        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                        if !ptr.is_null() {
                            let should_redraw = {
                                let state = &mut *ptr;
                                if !state.submitting.load(Ordering::Acquire) {
                                    let (x, y) = pointer_pos(lparam);
                                    let cx = x.clamp(0, state.width);
                                    let cy = y.clamp(0, state.height);
                                    state.anchor = Some((cx, cy));
                                    state.selection = Some(PixelRect::from_points(
                                        cx,
                                        cy,
                                        cx,
                                        cy,
                                        state.width,
                                        state.height,
                                    ));
                                    true
                                } else {
                                    false
                                }
                            };
                            if should_redraw {
                                let _ = InvalidateRect(hwnd, None, false);
                            }
                        }
                    }
                    LRESULT(0)
                }
                WM_MOUSEMOVE => {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        let updated = {
                            let state = &mut *ptr;
                            if let Some((ax, ay)) = state.anchor {
                                let (x, y) = pointer_pos(lparam);
                                state.selection = Some(PixelRect::from_points(
                                    ax,
                                    ay,
                                    x,
                                    y,
                                    state.width,
                                    state.height,
                                ));
                                true
                            } else {
                                false
                            }
                        };
                        if updated {
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                    }
                    LRESULT(0)
                }
                WM_LBUTTONUP => {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        let drag_end = {
                            let state = &mut *ptr;
                            state.anchor.take().map(|(ax, ay)| {
                                let (x, y) = pointer_pos(lparam);
                                let rect =
                                    PixelRect::from_points(ax, ay, x, y, state.width, state.height);
                                if rect.meets_minimum(state.scale)
                                    && state
                                        .submitting
                                        .compare_exchange(
                                            false,
                                            true,
                                            Ordering::AcqRel,
                                            Ordering::Acquire,
                                        )
                                        .is_ok()
                                {
                                    state.selection = Some(rect);
                                    let image_rect = rect.to_image_rect(
                                        state.width,
                                        state.height,
                                        state.img_width,
                                        state.img_height,
                                    );
                                    Some((state.app.clone(), state.label.clone(), image_rect))
                                } else {
                                    state.selection = None;
                                    None
                                }
                            })
                        };
                        if let Some(submission) = drag_end {
                            if GetCapture() == hwnd {
                                let _ = ReleaseCapture();
                            }
                            let _ = InvalidateRect(hwnd, None, false);
                            if let Some((app, label, image_rect)) = submission {
                                let _ = UpdateWindow(hwnd);
                                tauri::async_runtime::spawn(async move {
                                    if let Err(err) =
                                        crate::windows::accept_selection(&app, &label, image_rect)
                                    {
                                        crate::windows::abort_capture(&app, err);
                                    }
                                });
                            }
                        }
                    }
                    LRESULT(0)
                }
                WM_CAPTURECHANGED => {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        let cleared = {
                            let state = &mut *ptr;
                            if state.anchor.take().is_some()
                                && !state.submitting.load(Ordering::Acquire)
                            {
                                state.selection = None;
                                true
                            } else {
                                false
                            }
                        };
                        if cleared {
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                    }
                    LRESULT(0)
                }
                WM_RBUTTONDOWN => LRESULT(0),
                WM_RBUTTONUP => {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        let (had_anchor, app) = {
                            let state = &mut *ptr;
                            (state.anchor.take().is_some(), state.app.clone())
                        };
                        if had_anchor && GetCapture() == hwnd {
                            let _ = ReleaseCapture();
                        }
                        cancel_async(app);
                    }
                    LRESULT(0)
                }
                WM_KEYDOWN => {
                    if wparam.0 == VK_ESCAPE.0 as usize {
                        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                        if !ptr.is_null() {
                            let (had_anchor, app) = {
                                let state = &mut *ptr;
                                (state.anchor.take().is_some(), state.app.clone())
                            };
                            if had_anchor && GetCapture() == hwnd {
                                let _ = ReleaseCapture();
                            }
                            cancel_async(app);
                        }
                        return LRESULT(0);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
                WM_CLOSE => {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        let app = (*ptr).app.clone();
                        cancel_async(app);
                    }
                    LRESULT(0)
                }
                WM_NCDESTROY => {
                    let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut OverlayWindow;
                    if !ptr.is_null() {
                        drop(Box::from_raw(ptr));
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    fn ensure_class_registered() -> Result<HINSTANCE> {
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        let instance = HINSTANCE(
            unsafe { GetModuleHandleW(None) }
                .map_err(|_| window_error())?
                .0,
        );
        let ok = *REGISTERED.get_or_init(|| unsafe {
            let cursor = LoadCursorW(HINSTANCE::default(), IDC_CROSS).unwrap_or_default();
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(selection_wnd_proc),
                hInstance: instance,
                hCursor: cursor,
                lpszClassName: w!("ScreenTranslateNativeSelection"),
                ..Default::default()
            };
            RegisterClassW(&class) != 0
        });
        if !ok {
            return Err(window_error());
        }
        Ok(instance)
    }

    pub fn show(
        app: &AppHandle,
        specs: Vec<MonitorOverlaySpec>,
        dark: bool,
        locale: UiLocale,
        stop: CancellationToken,
    ) -> Result<()> {
        if specs.is_empty() {
            return Ok(());
        }
        if stop.is_cancelled() {
            return Err(AppError::cancelled());
        }
        let generation = {
            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
            reg.generation = reg.generation.wrapping_add(1);
            reg.generation
        };
        // Pre-convert RGBA -> BGRA off the UI thread so main-thread window creation is instant.
        let prepared: Vec<(MonitorOverlaySpec, Vec<u8>)> = specs
            .into_iter()
            .map(|spec| {
                let bgra = rgba_to_bgra(spec.image.as_raw());
                (spec, bgra)
            })
            .collect();
        if stop.is_cancelled() {
            return Err(AppError::cancelled());
        }

        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handle = app.clone();
        app.run_on_main_thread(move || {
            if stop.is_cancelled() {
                let _ = tx.send(Err(AppError::cancelled()));
                return;
            }
            let stale_before_create = {
                let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
                reg.generation != generation
            };
            if stale_before_create {
                let _ = tx.send(Err(AppError::cancelled()));
                return;
            }

            struct CreatedOverlay {
                hwnd: HWND,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                contains_cursor: bool,
            }

            let submitting = Arc::new(AtomicBool::new(false));
            let result = (|| -> Result<Vec<CreatedOverlay>> {
                let instance = ensure_class_registered()?;
                let mut cursor_pos = POINT::default();
                let has_cursor = unsafe { GetCursorPos(&mut cursor_pos) }.is_ok();
                let mut created: Vec<CreatedOverlay> = Vec::with_capacity(prepared.len());

                for (spec, bg_bgra) in prepared {
                    let width = spec.width.max(120);
                    let height = spec.height.max(80);
                    let scale = spec.scale.max(1.0);
                    let (back_dc, back_bmp, old_bmp, back_bits) =
                        match unsafe { OverlayWindow::create_backbuffer(width, height) } {
                            Ok(parts) => parts,
                            Err(err) => {
                                let raw: Vec<isize> =
                                    created.iter().map(|c| c.hwnd.0 as isize).collect();
                                destroy_handles(&raw);
                                return Err(err);
                            }
                        };
                    let font_guide =
                        unsafe { create_ui_font((13.0 * scale).round() as i32, FW_MEDIUM.0) };
                    let font_kbd =
                        unsafe { create_ui_font((11.0 * scale).round() as i32, FW_SEMIBOLD.0) };
                    let font_dim =
                        unsafe { create_ui_font((13.0 * scale).round() as i32, FW_NORMAL.0) };

                    let state = Box::new(OverlayWindow {
                        app: handle.clone(),
                        label: spec.label,
                        width,
                        height,
                        img_width: spec.image.width(),
                        img_height: spec.image.height(),
                        scale,
                        dark,
                        locale,
                        bg_bgra,
                        back_dc,
                        back_bmp,
                        old_bmp,
                        back_bits,
                        font_guide,
                        font_kbd,
                        font_dim,
                        anchor: None,
                        selection: None,
                        submitting: Arc::clone(&submitting),
                    });

                    let hwnd = match unsafe {
                        CreateWindowExW(
                            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                            w!("ScreenTranslateNativeSelection"),
                            w!(""),
                            WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                            spec.x,
                            spec.y,
                            width,
                            height,
                            HWND::default(),
                            HMENU::default(),
                            instance,
                            None,
                        )
                    } {
                        Ok(hwnd) => hwnd,
                        Err(_) => {
                            drop(state);
                            let raw: Vec<isize> =
                                created.iter().map(|c| c.hwnd.0 as isize).collect();
                            destroy_handles(&raw);
                            return Err(window_error());
                        }
                    };

                    unsafe {
                        let disabled = 1_i32;
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_TRANSITIONS_FORCEDISABLED,
                            &disabled as *const _ as _,
                            std::mem::size_of::<i32>() as u32,
                        );
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
                    }

                    let contains_cursor = has_cursor
                        && cursor_pos.x >= spec.x
                        && cursor_pos.y >= spec.y
                        && cursor_pos.x < spec.x + width
                        && cursor_pos.y < spec.y + height;
                    created.push(CreatedOverlay {
                        hwnd,
                        x: spec.x,
                        y: spec.y,
                        width,
                        height,
                        contains_cursor,
                    });
                }
                Ok(created)
            })();

            match result {
                Ok(created) => {
                    let handles: Vec<isize> = created.iter().map(|c| c.hwnd.0 as isize).collect();
                    let previous = {
                        let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
                        if stop.is_cancelled() || reg.generation != generation {
                            drop(reg);
                            destroy_handles(&handles);
                            let _ = tx.send(Err(AppError::cancelled()));
                            return;
                        }
                        std::mem::replace(&mut reg.handles, handles.clone())
                    };
                    destroy_handles(&previous);

                    let mut focus_hwnd = created.first().map(|c| c.hwnd);
                    for item in &created {
                        if item.contains_cursor {
                            focus_hwnd = Some(item.hwnd);
                        }
                        unsafe {
                            let flags = if item.contains_cursor {
                                SWP_SHOWWINDOW
                            } else {
                                SWP_SHOWWINDOW | SWP_NOACTIVATE
                            };
                            let _ = SetWindowPos(
                                item.hwnd,
                                HWND_TOPMOST,
                                item.x,
                                item.y,
                                item.width,
                                item.height,
                                flags,
                            );
                            let _ = UpdateWindow(item.hwnd);
                        }
                    }
                    if let Some(hwnd) = focus_hwnd {
                        unsafe {
                            let _ = SetForegroundWindow(hwnd);
                            let _ = SetFocus(hwnd);
                            if let Ok(cross) = LoadCursorW(HINSTANCE::default(), IDC_CROSS) {
                                let _ = SetCursor(cross);
                            }
                        }
                    }
                    if tx.send(Ok(())).is_err() {
                        // Caller timed out; clean up if our handles are still active.
                        let owned = {
                            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
                            if reg.generation == generation {
                                reg.generation = reg.generation.wrapping_add(1);
                                std::mem::take(&mut reg.handles)
                            } else {
                                Vec::new()
                            }
                        };
                        destroy_handles(&owned);
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(err));
                }
            }
        })
        .map_err(|_| window_error())?;

        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| window_error())?
    }
}

#[cfg(windows)]
pub use platform::{MonitorOverlaySpec, close, close_and_wait, is_active, show};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_rect_clamps_reverse_drag_and_maps_to_image_pixels() {
        let rect = PixelRect::from_points(800, 500, -20, 100, 1920, 1080);
        assert_eq!(
            rect,
            PixelRect {
                x0: 0,
                y0: 100,
                x1: 800,
                y1: 500
            }
        );
        assert!(rect.meets_minimum(1.0));
        let mapped = rect.to_image_rect(1920, 1080, 1920, 1080);
        assert_eq!(
            mapped,
            Rect {
                x: 0.0,
                y: 100.0,
                width: 800.0,
                height: 400.0,
            }
        );
        let tiny = PixelRect::from_points(10, 10, 13, 13, 1920, 1080);
        assert!(!tiny.meets_minimum(1.0));
    }

    #[test]
    fn rgba_to_bgra_swaps_red_and_blue_and_forces_opaque_alpha() {
        let bgra = rgba_to_bgra(&[0x12, 0x34, 0x56, 0x78, 0xff, 0x00, 0x80, 0x00]);
        assert_eq!(bgra, vec![0x56, 0x34, 0x12, 255, 0x80, 0x00, 0xff, 255]);
    }

    #[test]
    fn checked_bgra_len_rejects_zero_negative_and_overflow() {
        assert_eq!(checked_bgra_len(1920, 1080), Some(1920 * 1080 * 4));
        assert_eq!(checked_bgra_len(0, 1080), None);
        assert_eq!(checked_bgra_len(-1, 1080), None);
        assert_eq!(checked_bgra_len(i32::MAX, i32::MAX), None);
    }

    #[test]
    fn copy_bgra_surface_handles_padded_small_frames() {
        let src = vec![0xaa; 2 * 2 * 4];
        let mut dst = vec![0x55; 4 * 3 * 4];
        copy_bgra_surface(&mut dst, 4, 3, &src, 2, 2);
        // Top-left 2x2 copied, padded areas zeroed.
        assert_eq!(&dst[0..8], &[0xaa; 8]);
        assert_eq!(&dst[8..16], &[0x00; 8]);
        assert_eq!(&dst[16..24], &[0xaa; 8]);
        assert_eq!(&dst[32..48], &[0x00; 16]);
    }

    #[test]
    fn dimensions_badge_flips_inside_near_top_edge_and_clamps_right() {
        let near_top = PixelRect {
            x0: 1850,
            y0: 4,
            x1: 1910,
            y1: 100,
        };
        let (l, t, r, b) = dimensions_badge_rect(near_top, 1920, 1080, 90, 24, 1.0);
        assert!(t >= 4);
        assert!(r <= 1920);
        assert_eq!(r - l, 90);
        assert_eq!(b - t, 24);

        let middle = PixelRect {
            x0: 200,
            y0: 300,
            x1: 500,
            y1: 500,
        };
        let (l2, t2, _, b2) = dimensions_badge_rect(middle, 1920, 1080, 90, 24, 1.0);
        assert_eq!(l2, 200);
        assert!(b2 < 300 && t2 > 0);
    }

    #[test]
    fn selection_frame_and_rounded_box_render_within_buffer_bounds() {
        let mut buf = vec![0u8; 200 * 120 * 4];
        let mut canvas = Canvas::new(&mut buf, 200, 120);
        canvas.draw_selection_frame(
            PixelRect {
                x0: 20,
                y0: 20,
                x1: 120,
                y1: 80,
            },
            1.0,
        );
        let idx = ((19 * 200) + 50) * 4;
        assert_eq!(&canvas.buf[idx..idx + 4], &[0xff, 0x8b, 0x16, 255]);
        let corner = ((20 * 200) + 20) * 4;
        assert_eq!(&canvas.buf[corner..corner + 4], &[255, 255, 255, 255]);

        canvas.draw_rounded_box(
            (10.0, 10.0, 190.0, 46.0),
            RoundedBoxStyle {
                radius: 18.0,
                fill: (255, 255, 255, 0.92),
                border: Some((0, 0, 0, 0.1, 1.0)),
                shadow: Some((0.14, 12.0, 4.0)),
            },
        );
        canvas.draw_scan_icon(28.0, 28.0, 1.0, (0x52, 0x52, 0x5b));
    }

    #[test]
    fn rounded_box_shadow_falls_off_monotonically_below_pill() {
        let width = 200;
        let height = 100;
        let mut buf = vec![255u8; width * height * 4];
        let mut canvas = Canvas::new(&mut buf, width as i32, height as i32);
        canvas.draw_rounded_box(
            (20.0, 16.0, 180.0, 50.0),
            RoundedBoxStyle {
                radius: 17.0,
                fill: (255, 255, 255, 0.92),
                border: Some((0, 0, 0, 0.10, 1.0)),
                shadow: Some((0.14, 14.0, 4.0)),
            },
        );
        // Vertically below the pill center (x = 100, y = 51..72): brightness must
        // stay darker than white near the pill edge and monotonically increase
        // back to 255 with no hollow bright gap or sudden dark ring at y = 54.
        let mut prev = buf[(51 * width + 100) * 4];
        assert!(prev < 245, "shadow should start right below pill: {prev}");
        for y in 52..72 {
            let cur = buf[(y * width + 100) * 4];
            assert!(
                cur >= prev,
                "shadow brightness dropped at y={y} ({prev} -> {cur}), indicating a ring artifact"
            );
            prev = cur;
        }
        assert_eq!(buf[(71 * width + 100) * 4], 255);
        // Pill interior must remain untouched by the shadow.
        assert_eq!(buf[(33 * width + 100) * 4], 255);
    }
}
