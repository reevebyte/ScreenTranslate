// A small owned native button anchored in the existing caption; no client-space toolbar.
use std::cell::{Cell, RefCell};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{Dwm::*, Gdi::*},
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};
const SUBCLASS_ID: usize = 0x4355_5550;
const ANIMATION_TIMER: usize = 1;
#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptionMode {
    Download,
    Downloading,
    Restart,
}
fn caption_label(mode: CaptionMode, progress: Option<u32>, english: bool) -> String {
    match mode {
        CaptionMode::Download => if english { "Update" } else { "下载更新" }.into(),
        CaptionMode::Restart => if english { "Restart" } else { "重启更新" }.into(),
        CaptionMode::Downloading => progress
            .map(|n| format!("{} {}%", if english { "Get" } else { "下载" }, n.min(99)))
            .unwrap_or_else(|| {
                if english {
                    "Getting…"
                } else {
                    "下载中…"
                }
                .into()
            }),
    }
}
const WM_MOUSELEAVE: u32 = 0x02a3;
struct CaptionAction {
    owner: HWND,
    button: Cell<HWND>,
    visible: Cell<bool>,
    enabled: Cell<bool>,
    english: Cell<bool>,
    hover: Cell<bool>,
    pressed: Cell<bool>,
    close_pressed: Cell<bool>,
    close_hover: Cell<bool>,
    version: RefCell<String>,
    caption: Cell<u32>,
    text: Cell<u32>,
    mode: Cell<CaptionMode>,
    progress: Cell<Option<u32>>,
    frame: Cell<u32>,
    animating: Cell<bool>,
    clicked: Box<dyn Fn()>,
    dismissed: Box<dyn Fn()>,
}
fn action_rect(buttons: RECT, dpi: u32) -> Option<RECT> {
    let scale = dpi.max(96) as f64 / 96.;
    let gap = (6. * scale).round() as i32;
    let width = (104. * scale).round() as i32;
    let height = (24. * scale).round() as i32;
    if buttons.right <= buttons.left
        || buttons.bottom - buttons.top < height
        || buttons.left - gap - width < (100. * scale) as i32
    {
        return None;
    }
    let top = buttons.top + (buttons.bottom - buttons.top - height) / 2;
    Some(RECT {
        left: buttons.left - gap - width,
        top,
        right: buttons.left - gap,
        bottom: top + height,
    })
}
fn dismiss_rect(width: i32, dpi: u32) -> RECT {
    let size = (14. * dpi.max(96) as f64 / 96.).round() as i32;
    RECT {
        left: width - size,
        top: 0,
        right: width,
        bottom: size,
    }
}
fn pill_rect(width: i32, dpi: u32) -> RECT {
    let scale = dpi.max(96) as f64 / 96.;
    let px = |value: f64| (value * scale).round() as i32;
    RECT {
        left: 0,
        top: px(2.),
        right: width - px(16.),
        bottom: px(22.),
    }
}
fn contains(rect: RECT, l: LPARAM) -> bool {
    let x = l.0 as u16 as i16 as i32;
    let y = (l.0 >> 16) as u16 as i16 as i32;
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}
unsafe fn over_action(button: HWND, action: &CaptionAction, l: LPARAM) -> bool {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(button, &mut client);
        contains(pill_rect(client.right, GetDpiForWindow(action.owner)), l)
    }
}
unsafe fn over_dismiss(button: HWND, action: &CaptionAction, l: LPARAM) -> bool {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(button, &mut client);
        let rect = dismiss_rect(client.right, GetDpiForWindow(action.owner));
        contains(rect, l)
    }
}
unsafe fn state(hwnd: HWND) -> Option<&'static CaptionAction> {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let mut ptr = 0;
        if GetWindowSubclass(hwnd, Some(caption_proc), SUBCLASS_ID, Some(&mut ptr)).as_bool()
            && ptr != 0
        {
            Some(&*(ptr as *const CaptionAction))
        } else {
            None
        }
    }
}
unsafe fn bounds(hwnd: HWND) -> Option<RECT> {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let mut outer = RECT::default();
        GetWindowRect(hwnd, &mut outer).ok()?;
        let mut info = TITLEBARINFOEX {
            cbSize: std::mem::size_of::<TITLEBARINFOEX>() as u32,
            ..Default::default()
        };
        SendMessageW(
            hwnd,
            WM_GETTITLEBARINFOEX,
            WPARAM(0),
            LPARAM(&mut info as *mut _ as isize),
        );
        let min = info.rgrect[2];
        if min.right > min.left && min.bottom > min.top && info.rgstate[2] & 0x8000 == 0 {
            return action_rect(
                RECT {
                    left: min.left - outer.left,
                    top: min.top - outer.top,
                    right: min.right - outer.left,
                    bottom: min.bottom - outer.top,
                },
                GetDpiForWindow(hwnd),
            );
        }
        let mut buttons = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_BUTTON_BOUNDS,
            &mut buttons as *mut _ as _,
            16,
        )
        .ok()?;
        // Some DWM implementations report only the close button. Account for the other native controls.
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let count =
            1 + i32::from(style & WS_MINIMIZEBOX.0 != 0) + i32::from(style & WS_MAXIMIZEBOX.0 != 0);
        if buttons.right - buttons.left < 70 * GetDpiForWindow(hwnd) as i32 / 96 {
            buttons.left -= (count - 1) * (buttons.right - buttons.left);
        }
        action_rect(buttons, GetDpiForWindow(hwnd))
    }
}
// Rasterize the same rounded 1.5 DIP strokes at the actual monitor DPI.
// Distance coverage keeps small curves smooth without another graphics dependency.
#[allow(clippy::too_many_arguments)] // GDI paint inputs form one fixed icon layout.
unsafe fn paint_icon(
    dc: HDC,
    cx: i32,
    cy: i32,
    scale: f64,
    mode: CaptionMode,
    frame: u32,
    foreground: u32,
    background: u32,
) {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let size = (16. * scale).round() as i32;
        let scale = size as f64 / 16.;
        let mut strokes = Vec::<(f64, f64, f64, f64, f64)>::new();
        let mut path = |points: &[(f64, f64)]| {
            for pair in points.windows(2) {
                strokes.push((pair[0].0, pair[0].1, pair[1].0, pair[1].1, 1.));
            }
        };
        match mode {
            CaptionMode::Download => {
                path(&[(8., 2.5), (8., 9.5)]);
                path(&[(4.75, 6.25), (8., 9.5), (11.25, 6.25)]);
                path(&[
                    (2.75, 9.5),
                    (2.75, 11.75),
                    (2.9, 12.4),
                    (3.4, 12.9),
                    (4., 13.),
                    (12., 13.),
                    (12.6, 12.9),
                    (13.1, 12.4),
                    (13.25, 11.75),
                    (13.25, 9.5),
                ]);
            }
            CaptionMode::Restart => {
                // Two balanced circular arrows stay recognizable at caption size.
                for start in [190_f64, 10.] {
                    let points: Vec<_> = (0..=30)
                        .map(|i| {
                            let angle = (start + 150. * i as f64 / 30.).to_radians();
                            (8. + 5. * angle.cos(), 8. + 5. * angle.sin())
                        })
                        .collect();
                    path(&points);
                }
                path(&[(9.45, 6.29), (12.70, 6.29), (12.70, 3.04)]);
                path(&[(6.55, 9.71), (3.30, 9.71), (3.30, 12.96)]);
            }
            CaptionMode::Downloading => {
                for i in 0..48 {
                    let angle = (frame % 12) as f64 * std::f64::consts::TAU / 12.
                        + i as f64 * std::f64::consts::TAU / 64.;
                    let next = angle + std::f64::consts::TAU / 64.;
                    strokes.push((
                        8. + 5. * angle.cos(),
                        8. + 5. * angle.sin(),
                        8. + 5. * next.cos(),
                        8. + 5. * next.sin(),
                        0.2 + 0.8 * i as f64 / 47.,
                    ));
                }
            }
        }
        let mut pixels = vec![0_u8; (size * size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let px = (x as f64 + 0.5) / scale;
                let py = (y as f64 + 0.5) / scale;
                let mut coverage = 0_f64;
                for &(ax, ay, bx, by, opacity) in &strokes {
                    let vx = bx - ax;
                    let vy = by - ay;
                    let t = (((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy)).clamp(0., 1.);
                    let distance = (px - ax - t * vx).hypot(py - ay - t * vy);
                    coverage =
                        coverage.max(((0.75 - distance) * scale + 0.5).clamp(0., 1.) * opacity);
                }
                let index = ((y * size + x) * 4) as usize;
                for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
                    let bg = ((background >> shift) & 255) as f64;
                    let fg = ((foreground >> shift) & 255) as f64;
                    pixels[index + channel] = (bg + (fg - bg) * coverage).round() as u8;
                }
            }
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size;
        info.bmiHeader.biHeight = -size;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        SetDIBitsToDevice(
            dc,
            cx - size / 2,
            cy - size / 2,
            size as u32,
            size as u32,
            0,
            0,
            0,
            size as u32,
            pixels.as_ptr() as _,
            &info,
            DIB_RGB_COLORS,
        );
    }
}
unsafe fn paint_dc(dc: HDC, width: i32, height: i32, dpi: u32, action: &CaptionAction) {
    // SAFETY: Caller supplies a valid target DC for the given pixel dimensions.
    unsafe {
        let caption = action.caption.get();
        let text = action.text.get();
        let blend = |a: u32, b: u32, f: f64| -> u32 {
            (0..3)
                .map(|i| {
                    let s = i * 8;
                    let x = (a >> s) & 255;
                    let y = (b >> s) & 255;
                    ((x as f64 + (y as f64 - x as f64) * f).round() as u32) << s
                })
                .sum()
        };
        let scale = dpi.max(96) as f64 / 96.;
        let px = |n: f64| (n * scale).round() as i32;
        let loading = action.mode.get() == CaptionMode::Downloading;
        let pressed = action.pressed.get();
        let background = if action.enabled.get() && (action.hover.get() || pressed) {
            blend(caption, text, if pressed { 0.14 } else { 0.09 })
        } else if action.enabled.get() || loading {
            blend(caption, text, 0.04)
        } else {
            caption
        };
        let popup_width = width.max(1);
        let popup_height = height.max(1);
        let pill = pill_rect(popup_width, dpi);
        let foreground = if action.enabled.get() || loading {
            text
        } else {
            blend(caption, text, 0.42)
        };
        let border_color = blend(
            caption,
            text,
            if action.hover.get() && action.enabled.get() {
                0.24
            } else {
                0.15
            },
        );
        let stroke_w = scale.max(1.);
        let pcx = (pill.left + pill.right) as f64 * 0.5;
        let pcy = (pill.top + pill.bottom) as f64 * 0.5;
        let half_w = (pill.right - pill.left) as f64 * 0.5 - 0.5;
        let half_h = (pill.bottom - pill.top) as f64 * 0.5 - 0.5;
        let radius = (7. * scale).min(half_h).max(1.);
        let badge = dismiss_rect(popup_width, dpi);
        let bcx = (badge.left + badge.right) as f64 * 0.5;
        let bcy = (badge.top + badge.bottom) as f64 * 0.5;
        let br = (badge.right - badge.left) as f64 * 0.5 - 1.;
        let badge_bg = if action.close_hover.get() {
            blend(caption, text, 0.14)
        } else {
            blend(caption, text, 0.06)
        };
        let badge_border = blend(caption, text, 0.28);
        let cross_r = 2.2 * scale;
        let mut surface = vec![0_u8; (popup_width * popup_height * 4) as usize];
        for y in 0..popup_height {
            for x in 0..popup_width {
                let fx = x as f64 + 0.5;
                let fy = y as f64 + 0.5;
                let mut color = caption;
                let qx = (fx - pcx).abs() - (half_w - radius);
                let qy = (fy - pcy).abs() - (half_h - radius);
                let d_pill = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius;
                let fill_cov = (0.5 - d_pill).clamp(0., 1.);
                if fill_cov > 0. {
                    color = blend(color, background, fill_cov);
                }
                let d_border = (d_pill + 0.5 * stroke_w).abs() - 0.5 * stroke_w;
                let border_cov = (0.5 - d_border).clamp(0., 1.);
                if border_cov > 0. {
                    color = blend(color, border_color, border_cov);
                }
                if action.hover.get() {
                    let d_badge = (fx - bcx).hypot(fy - bcy) - br;
                    let b_fill = (0.5 - d_badge).clamp(0., 1.);
                    if b_fill > 0. {
                        color = blend(color, badge_bg, b_fill);
                    }
                    let d_b_border = (d_badge + 0.5 * stroke_w).abs() - 0.5 * stroke_w;
                    let b_border = (0.5 - d_b_border).clamp(0., 1.);
                    if b_border > 0. {
                        color = blend(color, badge_border, b_border);
                    }
                    if d_badge < 0. {
                        let ux = fx - bcx;
                        let uy = fy - bcy;
                        let mut x_cov = 0_f64;
                        for (sx, sy) in [(1., 1.), (1., -1.)] {
                            let t = ((ux * sx + uy * sy) / (2. * cross_r)).clamp(-1., 1.);
                            let dist_seg = (ux - t * sx * cross_r).hypot(uy - t * sy * cross_r);
                            x_cov = x_cov.max(((0.55 * scale - dist_seg) + 0.5).clamp(0., 1.));
                        }
                        if x_cov > 0. {
                            color = blend(color, text, x_cov * 0.85);
                        }
                    }
                }
                let idx = ((y * popup_width + x) * 4) as usize;
                surface[idx] = ((color >> 16) & 255) as u8;
                surface[idx + 1] = ((color >> 8) & 255) as u8;
                surface[idx + 2] = (color & 255) as u8;
            }
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = popup_width;
        info.bmiHeader.biHeight = -popup_height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        SetDIBitsToDevice(
            dc,
            0,
            0,
            popup_width as u32,
            popup_height as u32,
            0,
            0,
            0,
            popup_height as u32,
            surface.as_ptr() as _,
            &info,
            DIB_RGB_COLORS,
        );
        let center_x = px(13.);
        let center_y = (pill.top + pill.bottom) / 2;
        paint_icon(
            dc,
            center_x,
            center_y,
            scale,
            action.mode.get(),
            action.frame.get(),
            foreground,
            background,
        );
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(foreground));
        let font_height = -(12. * dpi.max(96) as f64 / 96.).round() as i32;
        let font = CreateFontW(
            font_height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32,
            w!("Microsoft YaHei UI"),
        );
        let old = SelectObject(
            dc,
            if font.0.is_null() {
                GetStockObject(DEFAULT_GUI_FONT)
            } else {
                font.into()
            },
        );
        let label = caption_label(
            action.mode.get(),
            action.progress.get(),
            action.english.get(),
        );
        let mut label = label.encode_utf16().collect::<Vec<_>>();
        let mut rect = pill;
        rect.left = px(24.);
        rect.right -= px(6.);
        DrawTextW(
            dc,
            &mut label,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        if !font.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}
unsafe fn paint(button: HWND, action: &CaptionAction) {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(button, &mut paint);
        let mut rect = RECT::default();
        let _ = GetClientRect(button, &mut rect);
        let width = rect.right.max(1);
        let height = rect.bottom.max(1);
        let mem_dc = CreateCompatibleDC(dc);
        if !mem_dc.0.is_null() {
            let mem_bitmap = CreateCompatibleBitmap(dc, width, height);
            if !mem_bitmap.0.is_null() {
                let old_bitmap = SelectObject(mem_dc, HGDIOBJ(mem_bitmap.0));
                paint_dc(mem_dc, width, height, GetDpiForWindow(action.owner), action);
                let _ = BitBlt(dc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
                SelectObject(mem_dc, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(mem_bitmap.0));
            } else {
                paint_dc(dc, width, height, GetDpiForWindow(action.owner), action);
            }
            let _ = DeleteDC(mem_dc);
        } else {
            paint_dc(dc, width, height, GetDpiForWindow(action.owner), action);
        }
        let _ = EndPaint(button, &paint);
    }
}
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    ptr: usize,
) -> LRESULT {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(button_proc), id);
            return DefSubclassProc(hwnd, message, w, l);
        }
        let action = &*(ptr as *const CaptionAction);
        match message {
            WM_SETTEXT => {
                let _ = DefWindowProcW(hwnd, message, w, l);
                return LRESULT(1);
            }
            WM_TIMER if w.0 == ANIMATION_TIMER => {
                action.frame.set(action.frame.get().wrapping_add(1));
                let _ = InvalidateRect(hwnd, None, false);
                return LRESULT(0);
            }
            WM_PAINT => {
                paint(hwnd, action);
                return LRESULT(0);
            }
            BM_CLICK => {
                if action.visible.get() && action.enabled.get() {
                    (action.clicked)();
                }
                return LRESULT(0);
            }
            WM_LBUTTONDOWN => {
                let close = over_dismiss(hwnd, action, l);
                let primary = over_action(hwnd, action, l);
                if action.visible.get() && ((action.enabled.get() && primary) || close) {
                    action.pressed.set(true);
                    action.close_pressed.set(close);
                    action.hover.set(true);
                    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let pressed = action.pressed.replace(false);
                let close = action.close_pressed.replace(false);
                let over_close = over_dismiss(hwnd, action, l);
                let x = l.0 as u16 as i16 as i32;
                let y = (l.0 >> 16) as u16 as i16 as i32;
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let inside = x >= 0 && x < rect.right && y >= 0 && y < rect.bottom;
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                let _ = InvalidateRect(hwnd, None, false);
                if pressed && inside && action.visible.get() {
                    if close && over_close {
                        action.visible.set(false);
                        sync(action.owner, action);
                        (action.dismissed)();
                    } else if !close
                        && !over_close
                        && action.enabled.get()
                        && over_action(hwnd, action, l)
                    {
                        (action.clicked)();
                    }
                }
                return LRESULT(0);
            }
            WM_CAPTURECHANGED => {
                action.pressed.set(false);
                action.close_pressed.set(false);
                action.close_hover.set(false);
                action.hover.set(false);
                let _ = InvalidateRect(hwnd, None, false);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                let x = l.0 as u16 as i16 as i32;
                let y = (l.0 >> 16) as u16 as i16 as i32;
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let inside = x >= 0 && x < rect.right && y >= 0 && y < rect.bottom;
                let close = over_dismiss(hwnd, action, l);
                let changed_close = action.close_hover.replace(close) != close;
                if action.hover.replace(inside) != inside || changed_close {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            WM_MOUSELEAVE => {
                action.hover.set(false);
                action.close_hover.set(false);
                let _ = InvalidateRect(hwnd, None, false);
            }
            _ => {}
        }
        let result = DefSubclassProc(hwnd, message, w, l);
        if matches!(
            message,
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_ENABLE | WM_CAPTURECHANGED
        ) {
            let _ = InvalidateRect(hwnd, None, false);
        }
        result
    }
}
/// Inherit and verify capture protection before showing the auxiliary caption control.
pub fn inherit_capture(owner: HWND) -> Result<(), String> {
    unsafe {
        let Some(action) = state(owner) else {
            return Ok(());
        };
        let button = action.button.get();
        if button.0.is_null() {
            return Ok(());
        }
        let mut expected = 0;
        let mut actual = 0;
        GetWindowDisplayAffinity(owner, &mut expected).map_err(|_| "无法读取标题栏共享保护")?;
        let result = (|| {
            SetWindowDisplayAffinity(button, WINDOW_DISPLAY_AFFINITY(expected))
                .map_err(|_| "无法同步标题栏共享保护")?;
            GetWindowDisplayAffinity(button, &mut actual).map_err(|_| "无法核验标题栏共享保护")?;
            if expected != actual {
                return Err("标题栏共享保护核验不符");
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = ShowWindow(button, SW_HIDE);
        }
        result.map_err(str::to_string)
    }
}
unsafe fn sync(owner: HWND, action: &CaptionAction) {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let button = action.button.get();
        if button.0.is_null() {
            return;
        }
        if !action.visible.get() {
            if action.pressed.replace(false) {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            }
            let _ = KillTimer(button, ANIMATION_TIMER);
            action.animating.set(false);
            let _ = ShowWindow(button, SW_HIDE);
            return;
        }
        let Some(rect) = bounds(owner) else {
            if action.pressed.replace(false) {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            }
            let _ = KillTimer(button, ANIMATION_TIMER);
            action.animating.set(false);
            let _ = ShowWindow(button, SW_HIDE);
            return;
        };
        if inherit_capture(owner).is_err() {
            return;
        }
        let mut outer = RECT::default();
        if GetWindowRect(owner, &mut outer).is_err() {
            return;
        }
        let target_x = outer.left + rect.left;
        let target_y = outer.top + rect.top;
        let target_w = rect.right - rect.left;
        let target_h = rect.bottom - rect.top;
        let mut current_rect = RECT::default();
        let _ = GetWindowRect(button, &mut current_rect);
        let same_pos = IsWindowVisible(button).as_bool()
            && current_rect.left == target_x
            && current_rect.top == target_y
            && (current_rect.right - current_rect.left) == target_w
            && (current_rect.bottom - current_rect.top) == target_h;
        if !same_pos {
            let _ = SetWindowPos(
                button,
                None,
                target_x,
                target_y,
                target_w,
                target_h,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            );
        }
        if action.mode.get() == CaptionMode::Downloading {
            let mut animate = windows::Win32::Foundation::BOOL(1);
            let _ = SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some(&mut animate as *mut _ as _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            if animate.as_bool() {
                if !action.animating.get() {
                    action
                        .animating
                        .set(SetTimer(button, ANIMATION_TIMER, 90, None) != 0);
                }
            } else {
                let _ = KillTimer(button, ANIMATION_TIMER);
                action.animating.set(false);
            }
        } else {
            let _ = KillTimer(button, ANIMATION_TIMER);
            action.animating.set(false);
        }
        let _ = InvalidateRect(button, None, false);
    }
}
unsafe extern "system" fn caption_proc(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    ptr: usize,
) -> LRESULT {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let action = &*(ptr as *const CaptionAction);
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(caption_proc), id);
            let button = action.button.replace(HWND::default());
            if !button.0.is_null() {
                let _ = DestroyWindow(button);
            }
            let result = DefSubclassProc(hwnd, message, w, l);
            drop(Box::from_raw(ptr as *mut CaptionAction));
            return result;
        }
        let result = DefSubclassProc(hwnd, message, w, l);
        // Keep the native client rectangle calculated by DefSubclassProc, but
        // do not copy old right/bottom-edge pixels into the resized WebView.
        if message == WM_NCCALCSIZE && w.0 != 0 {
            return LRESULT(WVR_REDRAW as isize);
        }
        if matches!(
            message,
            WM_WINDOWPOSCHANGED | WM_SIZE | WM_DPICHANGED | WM_SHOWWINDOW | WM_THEMECHANGED
        ) {
            sync(hwnd, action);
        }
        result
    }
}
unsafe fn attach(
    hwnd: HWND,
    clicked: Box<dyn Fn()>,
    dismissed: Box<dyn Fn()>,
) -> Result<(), String> {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        if state(hwnd).is_some() {
            return Ok(());
        }
        let ptr = Box::into_raw(Box::new(CaptionAction {
            owner: hwnd,
            button: Cell::new(HWND::default()),
            visible: Cell::new(false),
            enabled: Cell::new(false),
            english: Cell::new(false),
            hover: Cell::new(false),
            pressed: Cell::new(false),
            close_pressed: Cell::new(false),
            close_hover: Cell::new(false),
            version: RefCell::new(String::new()),
            caption: Cell::new(0x00f1eef0),
            text: Cell::new(0x00272120),
            mode: Cell::new(CaptionMode::Download),
            progress: Cell::new(None),
            frame: Cell::new(0),
            animating: Cell::new(false),
            clicked,
            dismissed,
        }));
        if !SetWindowSubclass(hwnd, Some(caption_proc), SUBCLASS_ID, ptr as usize).as_bool() {
            drop(Box::from_raw(ptr));
            return Err("无法添加标题栏更新按钮".into());
        }
        let result = (|| {
            let button = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("BUTTON"),
                w!("下载更新"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                hwnd,
                None,
                None,
                None,
            )
            .map_err(|_| "无法创建标题栏更新按钮")?;
            (*ptr).button.set(button);
            if !SetWindowSubclass(button, Some(button_proc), SUBCLASS_ID, ptr as usize).as_bool() {
                return Err("无法绘制标题栏更新按钮");
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = RemoveWindowSubclass(hwnd, Some(caption_proc), SUBCLASS_ID);
            let button = (*ptr).button.get();
            if !button.0.is_null() {
                let _ = DestroyWindow(button);
            }
            drop(Box::from_raw(ptr));
        }
        result.map_err(str::to_string)
    }
}
unsafe fn update(
    hwnd: HWND,
    visible: bool,
    enabled: bool,
    mode: CaptionMode,
    progress: Option<u32>,
) {
    // SAFETY: Called on the owning UI thread; subclass state lives until WM_NCDESTROY.
    unsafe {
        let Some(action) = state(hwnd) else {
            return;
        };
        action.visible.set(visible);
        let enabled = enabled && mode != CaptionMode::Downloading;
        action.enabled.set(enabled);
        action.mode.set(mode);
        action.progress.set(progress.map(|n| n.min(99)));
        let label = caption_label(mode, progress, action.english.get());
        let label: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
        let _ = SetWindowTextW(action.button.get(), windows::core::PCWSTR(label.as_ptr()));
        if !visible {
            action.hover.set(false);
            action.close_hover.set(false);
        }
        if (!visible || (!enabled && !action.close_pressed.get())) && action.pressed.replace(false)
        {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
        }
        let _ =
            windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(action.button.get(), true);
        sync(hwnd, action);
    }
}
/// Called on the UI thread along the existing native caption theme clock.
pub fn refresh(raw: *mut std::ffi::c_void, caption: u32, text: u32) {
    unsafe {
        let owner = HWND(raw);
        if let Some(action) = state(owner) {
            action.caption.set(caption.min(0xffffff));
            action.text.set(text.min(0xffffff));
            let button = action.button.get();
            if !button.0.is_null() {
                let _ = InvalidateRect(button, None, false);
                let _ = UpdateWindow(button);
            }
        }
    }
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Native command mirrors the small caption state.
pub async fn set_update_caption(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    visible: bool,
    enabled: bool,
    mode: Option<String>,
    progress: Option<u32>,
    version: Option<String>,
    english: Option<bool>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请在主窗口管理软件更新".into());
    }
    let version = version.unwrap_or_default();
    if version.len() > 64 || version.chars().any(char::is_control) {
        return Err("无效的更新版本".into());
    }
    let mode = match mode.as_deref().unwrap_or("download") {
        "download" => CaptionMode::Download,
        "downloading" => CaptionMode::Downloading,
        "restart" => CaptionMode::Restart,
        _ => return Err("无效的更新按钮状态".into()),
    };
    crate::update_ui::on_ui(&app, move |app| {
        use tauri::{Emitter, Manager};
        let window = app.get_webview_window("main").ok_or("主窗口不存在")?;
        let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
        unsafe {
            if visible && state(hwnd).is_none() {
                let handle = app.clone();
                let dismissed_handle = app.clone();
                attach(
                    hwnd,
                    Box::new(move || {
                        crate::update_ui::request(&handle, "caption");
                    }),
                    Box::new(move || {
                        if let Some(action) = state(hwnd) {
                            let _ = dismissed_handle.emit_to(
                                "main",
                                "app-update-caption-dismissed",
                                action.version.borrow().clone(),
                            );
                        }
                    }),
                )?;
            }
            let (caption, text) = crate::update_ui::caption_colors(&app);
            if let Some(action) = state(hwnd) {
                action.caption.set(caption.min(0xffffff));
                action.text.set(text.min(0xffffff));
                action.version.replace(version);
                action.english.set(english.unwrap_or(false));
            }
            update(hwnd, visible, enabled, mode, progress);
        }
        Ok(())
    })
    .await
}
