use crate::{
    pipeline,
    state::{AppState, lock},
};
use screentrans_core::{
    AppError, CaptureFrame, ErrorCode, FrameMetadata, Rect, Result, ports::CaptureBackend,
};
use screentrans_platform::capture::SystemCapture;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

pub(crate) fn window_error() -> AppError {
    AppError::new(ErrorCode::Internal, "无法创建应用窗口，请重试")
}

#[derive(Default)]
pub struct WindowLifecycle {
    ready: HashSet<String>,
    pending: HashSet<String>,
    painted: HashSet<String>,
}

impl WindowLifecycle {
    fn request(&mut self, label: &str) -> bool {
        if self.ready.contains(label) {
            return true;
        }
        self.pending.insert(label.into());
        false
    }
    fn loaded(&mut self, label: &str) -> bool {
        self.ready.insert(label.into());
        self.pending.remove(label)
    }
    fn forget(&mut self, label: &str) {
        self.ready.remove(label);
        self.pending.remove(label);
        self.painted.remove(label);
    }
}

fn reveal(window: &tauri::WebviewWindow) -> Result<()> {
    disable_transitions(window);
    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
    }
    window.show().map_err(|_| window_error())?;
    if window.label() != "result-tools"
        && (!window.label().starts_with("selection-") || contains_cursor(window))
    {
        let _ = window.set_focus();
    }
    #[cfg(target_os = "macos")]
    if window.label() == "main" {
        screentrans_platform::activate_app();
    } else {
        screentrans_platform::focus_app();
    }
    Ok(())
}

pub(crate) fn request_show(window: &tauri::WebviewWindow) -> Result<()> {
    let ready = lock(&window.state::<AppState>().window_lifecycle).request(window.label());
    if ready {
        reveal(window)?;
    }
    Ok(())
}

pub fn window_destroyed(window: &tauri::Window) {
    // Windows also destroys the owned toolbar when its image owner is reaped.
    lock(&window.state::<AppState>().window_lifecycle).forget(window.label());
}

pub fn window_ready(window: &tauri::WebviewWindow, frame_id: Option<&str>) -> Result<()> {
    let state = window.state::<AppState>();
    if window.label() == "result" {
        let current = lock(&state.sessions)
            .visual
            .frame
            .as_ref()
            .map(|f| f.id.clone());
        if current.as_deref() != frame_id {
            return Ok(());
        }
    }
    if window.label().starts_with("selection-") {
        let current = lock(&state.selections).get(window.label()).cloned();
        if current.as_deref() != frame_id {
            return Ok(());
        }
    }
    let pending = lock(&window.state::<AppState>().window_lifecycle).loaded(window.label());
    if window.label() == "result-tools"
        && (lock(&state.result_toolbar).suppressed()
            || !window
                .app_handle()
                .get_webview_window("result")
                .is_some_and(|w| w.is_visible().unwrap_or(false)))
    {
        return Ok(());
    }
    if pending {
        reveal(window)?;
    }
    Ok(())
}

pub fn result_presented(window: &tauri::WebviewWindow, frame_id: &str) {
    let state = window.state::<AppState>();
    let current = lock(&state.result_frame_id).clone();
    if window.label() == "result" && current == frame_id {
        flush_compositor();
        cancel_capture(window.app_handle(), false);
        lock(&state.frames).retain_current(frame_id);
        let _ = crate::result_tools::sync(window.app_handle());
    }
}

pub fn selection_presented(window: &tauri::WebviewWindow) {
    let state = window.state::<AppState>();
    let labels: Vec<_> = lock(&state.selections).keys().cloned().collect();
    if !labels.iter().any(|label| label == window.label()) {
        return;
    }
    let all_painted = {
        let mut lifecycle = lock(&state.window_lifecycle);
        lifecycle.painted.insert(window.label().into());
        labels.iter().all(|label| lifecycle.painted.contains(label))
    };
    if all_painted {
        flush_compositor();
    }
}

fn disable_transitions(window: &tauri::WebviewWindow) {
    if window.label() == "main" {
        return;
    }
    #[cfg(windows)]
    if let Ok(handle) = window.hwnd() {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute,
        };
        let disabled = 1_i32;
        unsafe {
            let _ = DwmSetWindowAttribute(
                windows::Win32::Foundation::HWND(handle.0),
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &disabled as *const _ as _,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
    let _ = window;
}

fn flush_compositor() {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
}

fn contains_cursor(window: &tauri::WebviewWindow) -> bool {
    #[cfg(windows)]
    {
        let mut point = windows::Win32::Foundation::POINT::default();
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.is_ok()
            && let (Ok(position), Ok(size)) = (window.inner_position(), window.inner_size())
        {
            return point.x >= position.x
                && point.y >= position.y
                && (point.x as i64) < position.x as i64 + size.width as i64
                && (point.y as i64) < position.y as i64 + size.height as i64;
        }
    }
    let _ = window;
    true
}

fn align_client(window: &tauri::WebviewWindow, x: f64, y: f64) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    if let (Ok(inner), Ok(outer)) = (window.inner_position(), window.outer_position()) {
        let dx = x.round() as i32 - inner.x;
        let dy = y.round() as i32 - inner.y;
        if dx != 0 || dy != 0 {
            window
                .set_position(PhysicalPosition::new(outer.x + dx, outer.y + dy))
                .map_err(|_| window_error())?;
        }
    }
    let _ = (window, x, y);
    Ok(())
}

fn native_theme(theme: screentrans_core::config::Theme) -> Option<tauri::Theme> {
    // Changing the Windows native theme repaints the entire WebView. Only update
    // DWM caption attributes; the document owns the application theme.
    if cfg!(windows) {
        return None;
    }
    match theme {
        screentrans_core::config::Theme::Light => Some(tauri::Theme::Light),
        screentrans_core::config::Theme::Dark => Some(tauri::Theme::Dark),
        screentrans_core::config::Theme::System => None,
    }
}
pub fn apply_theme(app: &AppHandle, theme: screentrans_core::config::Theme) {
    for window in app.webview_windows().values() {
        #[cfg(not(windows))]
        {
            let target = native_theme(theme);
            if target.is_none() || window.theme().ok() != target {
                let _ = window.set_theme(target);
            }
        }
        caption_color(window, theme);
    }
}

#[cfg(windows)]
pub fn caption_palette(theme: screentrans_core::config::Theme) -> (bool, u32, u32) {
    let dark = match theme {
        screentrans_core::config::Theme::Dark => true,
        screentrans_core::config::Theme::Light => false,
        screentrans_core::config::Theme::System => system_dark(),
    };
    let background = if dark { 0x00201c1d } else { 0x00f1eef0 };
    let text = if dark { 0x00f3edf0 } else { 0x00272120 };
    (dark, background, text)
}

pub fn theme_background_color(theme: screentrans_core::config::Theme) -> tauri::window::Color {
    let dark = match theme {
        screentrans_core::config::Theme::Dark => true,
        screentrans_core::config::Theme::Light => false,
        screentrans_core::config::Theme::System => system_dark(),
    };
    if dark {
        tauri::window::Color(0x1d, 0x1c, 0x20, 255)
    } else {
        tauri::window::Color(0xf0, 0xee, 0xf1, 255)
    }
}

fn caption_color(window: &tauri::WebviewWindow, theme: screentrans_core::config::Theme) {
    let _ = (window, theme);
    if window.label() != "main" {
        #[cfg(windows)]
        let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
        return;
    }
    #[cfg(windows)]
    if let Ok(handle) = window.hwnd() {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DwmSetWindowAttribute,
        };
        let (dark, background, text) = caption_palette(theme);
        unsafe {
            let hwnd = windows::Win32::Foundation::HWND(handle.0);
            crate::update_caption::refresh(handle.0, background, text);
            use windows::Win32::Graphics::Gdi::{DeleteObject, HBRUSH};
            use windows::Win32::UI::WindowsAndMessaging::{GCLP_HBRBACKGROUND, SetClassLongPtrW};
            let old = SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);
            if old != 0 {
                let _ = DeleteObject(HBRUSH(old as *mut _));
            }
            let bg_color = if dark {
                tauri::window::Color(0x1d, 0x1c, 0x20, 255)
            } else {
                tauri::window::Color(0xf0, 0xee, 0xf1, 255)
            };
            let _ = window.set_background_color(Some(bg_color));
            let immersive = i32::from(dark);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &immersive as *const _ as _,
                4,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &background as *const _ as _,
                std::mem::size_of::<u32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TEXT_COLOR,
                &text as *const _ as _,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(ns_window) = window.ns_window() {
        screentrans_platform::configure_window(ns_window, theme);
    }
    let _ = (window, theme);
}

fn system_dark() -> bool {
    #[cfg(windows)]
    {
        use windows::{
            Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
            core::w,
        };
        let mut light = 1u32;
        let mut size = 4u32;
        unsafe {
            let _ = RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut light as *mut _ as _),
                Some(&mut size),
            );
        }
        light == 0
    }
    #[cfg(target_os = "macos")]
    {
        screentrans_platform::system_dark()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

pub(crate) const RESULT_CONTROLS_HEIGHT: f64 = 48.0;
pub(crate) const RESULT_CONTROLS_WIDTH: f64 = 240.0;
pub(crate) const RESULT_PADDING: f64 = 8.0;

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultLayout {
    pub frame_id: String,
    pub controls_above: bool,
    pub image_align_left: bool,
    pub toolbar_align_left: bool,
    pub image_width: f64,
    pub image_height: f64,
}

fn result_geometry(bounds: Rect, scale: f64, work_area: Option<Rect>) -> (Rect, ResultLayout) {
    let controls_height = RESULT_CONTROLS_HEIGHT * scale;
    let mut layout = ResultLayout::default();
    if let Some(area) = work_area {
        layout.controls_above = bounds.bottom() + controls_height + RESULT_PADDING * scale
            > area.bottom()
            && bounds.y - controls_height >= area.y;
        layout.image_align_left = bounds.right()
            - bounds.width.max(RESULT_CONTROLS_WIDTH * scale)
            - RESULT_PADDING * scale
            < area.x;
    }
    (result_outer(bounds, scale, &layout), layout)
}

fn result_outer(bounds: Rect, scale: f64, layout: &ResultLayout) -> Rect {
    let padding = RESULT_PADDING * scale;
    let width = bounds.width.max(RESULT_CONTROLS_WIDTH * scale);
    Rect {
        x: if layout.image_align_left {
            bounds.x
        } else {
            bounds.right() - width
        } - padding,
        y: bounds.y
            - if layout.controls_above {
                RESULT_CONTROLS_HEIGHT * scale
            } else {
                0.0
            }
            - padding,
        width: width + 2.0 * padding,
        height: bounds.height + RESULT_CONTROLS_HEIGHT * scale + 2.0 * padding,
    }
}

pub fn set_result_toolbar(
    window: &tauri::WebviewWindow,
    above: bool,
    align_left: bool,
) -> Result<ResultLayout> {
    crate::result_tools::dock(window.app_handle(), above, align_left)
}

pub fn begin_result_resize(
    window: &tauri::WebviewWindow,
    direction: crate::result_resize::ResizeDirection,
) -> Result<()> {
    let state = window.state::<AppState>();
    let frame = lock(&state.sessions)
        .visual
        .frame
        .clone()
        .ok_or_else(window_error)?;
    let layout = lock(&state.result_layout).clone();
    let scale = window.scale_factor().map_err(|_| window_error())?;
    #[cfg(target_os = "linux")]
    let cursor = if screentrans_platform::is_wayland() {
        (0.0, 0.0)
    } else {
        let cursor = window.cursor_position().map_err(|_| window_error())?;
        (cursor.x, cursor.y)
    };
    #[cfg(not(target_os = "linux"))]
    let cursor = {
        let cursor = window.cursor_position().map_err(|_| window_error())?;
        (cursor.x, cursor.y)
    };
    let bounds = crate::result_tools::image_bounds(window)?;
    #[cfg(target_os = "linux")]
    let size = window.inner_size().map_err(|_| window_error())?;
    *lock(&state.result_resize) = Some(crate::result_resize::ResizeGesture {
        #[cfg(windows)]
        outline: crate::resize_outline::ResizeOutline::new(window, bounds, scale)?,
        frame_id: frame.id,
        bounds,
        current: bounds,
        cursor,
        direction,
        scale,
        #[cfg(target_os = "linux")]
        start_size: (size.width, size.height),
        layout,
    });
    {
        let mut toolbar = lock(&state.result_toolbar);
        toolbar.image_resizing = true;
        toolbar.gesture_generation += 1;
    }
    crate::result_tools::sync(window.app_handle())?;
    Ok(())
}

pub fn update_result_resize(
    window: &tauri::WebviewWindow,
    finish: bool,
    dx: Option<f64>,
    dy: Option<f64>,
) -> Result<Option<Rect>> {
    #[cfg(not(target_os = "linux"))]
    let _ = (dx, dy);
    let state = window.state::<AppState>();
    if finish {
        let bounds = lock(&state.result_resize)
            .take()
            .map(|gesture| gesture.current);
        crate::result_tools::finish_image_gesture(window.app_handle())?;
        return Ok(bounds);
    }
    let Some(gesture) = lock(&state.result_resize).clone() else {
        return Ok(None);
    };
    if *lock(&state.result_frame_id) != gesture.frame_id {
        *lock(&state.result_resize) = None;
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    let (delta_x, delta_y) = if screentrans_platform::is_wayland() {
        (
            dx.unwrap_or(0.0) * gesture.scale,
            dy.unwrap_or(0.0) * gesture.scale,
        )
    } else {
        let cursor = window.cursor_position().map_err(|_| window_error())?;
        (cursor.x - gesture.cursor.0, cursor.y - gesture.cursor.1)
    };
    #[cfg(not(target_os = "linux"))]
    let (delta_x, delta_y) = {
        let cursor = window.cursor_position().map_err(|_| window_error())?;
        (cursor.x - gesture.cursor.0, cursor.y - gesture.cursor.1)
    };
    let bounds = crate::result_resize::resize(
        gesture.bounds,
        gesture.direction,
        delta_x,
        delta_y,
        gesture.scale,
    );
    let mut layout = gesture.layout.clone();
    layout.image_width = bounds.width / gesture.scale;
    layout.image_height = bounds.height / gesture.scale;
    if bounds == gesture.current {
        return Ok(Some(bounds));
    }
    let outer = result_outer(bounds, gesture.scale, &layout);
    #[cfg(windows)]
    gesture
        .outline
        .update(window, outer, bounds, gesture.scale)?;
    #[cfg(not(windows))]
    set_client_rect(window, outer)?;
    *lock(&state.result_layout) = layout.clone();
    let _ = window.emit("result-layout-changed", layout);
    if let Some(active) = lock(&state.result_resize).as_mut()
        && active.frame_id == gesture.frame_id
    {
        active.current = bounds;
    }
    Ok(Some(bounds))
}

pub(crate) fn set_client_rect(window: &tauri::WebviewWindow, bounds: Rect) -> Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
        let handle = window.hwnd().map_err(|_| window_error())?;
        // Both callers are undecorated result windows. Their client and outer
        // rectangles coincide; four synchronous geometry reads per frame are wasteful.
        debug_assert!(matches!(window.label(), "result" | "result-tools"));
        unsafe {
            SetWindowPos(
                windows::Win32::Foundation::HWND(handle.0),
                None,
                bounds.x.round() as i32,
                bounds.y.round() as i32,
                bounds.width.round() as i32,
                bounds.height.round() as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|_| window_error())?;
        }
    }
    #[cfg(not(windows))]
    {
        window
            .set_size(PhysicalSize::new(
                bounds.width.round() as u32,
                bounds.height.round() as u32,
            ))
            .map_err(|_| window_error())?;
        #[cfg(target_os = "linux")]
        if screentrans_platform::is_wayland() {
            return Ok(());
        }
        let inner = window.inner_position().map_err(|_| window_error())?;
        let outer = window.outer_position().map_err(|_| window_error())?;
        window
            .set_position(PhysicalPosition::new(
                outer.x + bounds.x.round() as i32 - inner.x,
                outer.y + bounds.y.round() as i32 - inner.y,
            ))
            .map_err(|_| window_error())?;
    }
    Ok(())
}

fn place_result(
    window: &tauri::WebviewWindow,
    bounds: Rect,
    frame: &FrameMetadata,
) -> Result<ResultLayout> {
    #[cfg(target_os = "macos")]
    let scale = 1.0;
    #[cfg(not(target_os = "macos"))]
    let scale = frame.scale_factor.max(1.0);
    let _ = frame;
    let work_area = window.available_monitors().ok().and_then(|monitors| {
        monitors.into_iter().find_map(|monitor| {
            #[cfg(target_os = "macos")]
            let unit = monitor.scale_factor();
            #[cfg(not(target_os = "macos"))]
            let unit = 1.0;
            let position = monitor.position();
            let size = monitor.size();
            let center_x = bounds.x + bounds.width / 2.0;
            let center_y = bounds.y + bounds.height / 2.0;
            if center_x < position.x as f64 / unit
                || center_y < position.y as f64 / unit
                || center_x >= (position.x as f64 + size.width as f64) / unit
                || center_y >= (position.y as f64 + size.height as f64) / unit
            {
                return None;
            }
            let area = monitor.work_area();
            Some(Rect {
                x: area.position.x as f64 / unit,
                y: area.position.y as f64 / unit,
                width: area.size.width as f64 / unit,
                height: area.size.height as f64 / unit,
            })
        })
    });
    let (outer, layout) = result_geometry(bounds, scale, work_area);
    #[cfg(target_os = "macos")]
    {
        window
            .set_size(tauri::LogicalSize::new(outer.width, outer.height))
            .map_err(|_| window_error())?;
        window
            .set_position(tauri::LogicalPosition::new(outer.x, outer.y))
            .map_err(|_| window_error())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        window
            .set_position(PhysicalPosition::new(
                outer.x.round() as i32,
                outer.y.round() as i32,
            ))
            .map_err(|_| window_error())?;
        window
            .set_size(PhysicalSize::new(
                outer.width.round() as u32,
                outer.height.round() as u32,
            ))
            .map_err(|_| window_error())?;
    }
    align_client(window, outer.x, outer.y)?;
    Ok(layout)
}

fn place_on_desktop(
    window: &tauri::WebviewWindow,
    bounds: Rect,
    frame: &FrameMetadata,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = bounds.width.max(120.0);
        let h = bounds.height.max(80.0);
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
        let _ = window.set_position(tauri::LogicalPosition::new(bounds.x, bounds.y));
        let _ = frame;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let w = frame.width.max(120);
        let h = frame.height.max(80);
        let _ = window.set_position(PhysicalPosition::new(bounds.x as i32, bounds.y as i32));
        let _ = window.set_size(PhysicalSize::new(w, h));
        align_client(window, bounds.x, bounds.y)?;
    }
    Ok(())
}

pub(crate) fn builder<'a>(
    app: &'a AppHandle,
    label: &str,
    query: &str,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    lock(&app.state::<AppState>().window_lifecycle).forget(label);
    let theme = lock(&app.state::<AppState>().config).appearance.theme;
    let mut builder = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(
            if label.starts_with("selection-") {
                format!("selection.html?{query}")
            } else {
                format!("index.html?window={query}")
            }
            .into(),
        ),
    )
    .title(match label {
        "result" => "ScreenTranslate · 截图",
        "result-tools" => "ScreenTranslate · 工具条",
        _ => "ScreenTranslate",
    })
    .theme(native_theme(theme))
    .visible(false)
    .on_navigation(|url| {
        url.scheme() == "tauri"
            || matches!(
                url.host_str(),
                Some("tauri.localhost" | "127.0.0.1" | "localhost")
            )
    });
    if label == "main" {
        builder = builder.background_color(theme_background_color(theme));
        #[cfg(target_os = "macos")]
        {
            builder = builder
                .title_bar_style(tauri::TitleBarStyle::Transparent)
                .hidden_title(true);
        }
    } else {
        builder = builder.background_color(tauri::window::Color(0, 0, 0, 0));
    }
    builder
}

fn selection_window(app: &AppHandle, label: &str) -> Result<tauri::WebviewWindow> {
    let theme = lock(&app.state::<AppState>().config).appearance.theme;
    let theme_name = match theme {
        screentrans_core::config::Theme::Dark => "dark",
        screentrans_core::config::Theme::Light => "light",
        screentrans_core::config::Theme::System => "system",
    };
    builder(app, label, &format!("selection&theme={theme_name}"))
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        .map_err(|_| window_error())
}

pub fn prewarm_selection(app: &AppHandle) {
    #[cfg(not(windows))]
    let _ = app;
    #[cfg(windows)]
    if app.get_webview_window("selection-0").is_none() {
        let _ = selection_window(app, "selection-0");
    }
}

pub fn ensure_main(app: &AppHandle) -> Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("main") {
        return Ok(window);
    }
    let theme = lock(&app.state::<AppState>().config).appearance.theme;
    let window = builder(app, "main", "main")
        .inner_size(680.0, 520.0)
        .min_inner_size(680.0, 520.0)
        .center()
        .build()
        .map_err(|_| window_error())?;
    caption_color(&window, theme);
    Ok(window)
}

pub fn ensure_quick(app: &AppHandle) -> Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("quick") {
        return Ok(window);
    }
    let window = builder(app, "quick", "quick")
        .inner_size(600.0, 320.0)
        .min_inner_size(420.0, 260.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .build()
        .map_err(|_| window_error())?;
    Ok(window)
}

pub fn show_main(app: &AppHandle) -> Result<()> {
    #[cfg(target_os = "macos")]
    screentrans_platform::activate_app();
    let window = ensure_main(app)?;
    let theme = lock(&app.state::<AppState>().config).appearance.theme;
    caption_color(&window, theme);
    request_show(&window)
}

pub fn toggle_quick(app: &AppHandle) -> Result<()> {
    let window = ensure_quick(app)?;
    if window.is_visible().unwrap_or(false) {
        window.hide().map_err(|_| window_error())?;
    } else {
        request_show(&window)?;
        let _ = window.emit("focus-input", ());
    }
    Ok(())
}

pub fn show_result(app: &AppHandle, frame: &FrameMetadata) -> Result<()> {
    *lock(&app.state::<AppState>().result_resize) = None;
    let same_frame = *lock(&app.state::<AppState>().result_frame_id) == frame.id;
    let window = match app.get_webview_window("result") {
        Some(window)
            if same_frame
                && lock(&app.state::<AppState>().window_lifecycle)
                    .ready
                    .contains("result") =>
        {
            request_show(&window)?;
            crate::result_tools::sync(app)?;
            return Ok(());
        }
        Some(window) => window,
        None => builder(app, "result", "result")
            .inner_size(640.0, 400.0)
            .min_inner_size(
                RESULT_CONTROLS_WIDTH + 2.0 * RESULT_PADDING,
                RESULT_CONTROLS_HEIGHT + 2.0 * RESULT_PADDING + 1.0,
            )
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .resizable(cfg!(target_os = "linux"))
            .skip_taskbar(true)
            .build()
            .map_err(|_| window_error())?,
    };
    lock(&app.state::<AppState>().window_lifecycle).forget("result");
    let _ = window.hide();
    let _ = crate::result_tools::hide(app);
    if !screentrans_platform::is_wayland() {
        let _ = crate::result_tools::prepare(app)?;
    }
    *lock(&app.state::<AppState>().result_frame_id) = frame.id.clone();
    let overlay = lock(&app.state::<AppState>().capabilities).overlay;
    let mut layout = if let Some(bounds) = frame.screen_bounds.filter(|_| overlay) {
        place_result(&window, bounds, frame)?
    } else {
        let scale = (900.0 / frame.width as f64)
            .min(600.0 / frame.height as f64)
            .min(1.0);
        window
            .set_size(tauri::LogicalSize::new(
                (frame.width as f64 * scale).max(RESULT_CONTROLS_WIDTH) + 2.0 * RESULT_PADDING,
                frame.height as f64 * scale + RESULT_CONTROLS_HEIGHT + 2.0 * RESULT_PADDING,
            ))
            .map_err(|_| window_error())?;
        let _ = window.center();
        ResultLayout::default()
    };
    #[cfg(not(target_os = "macos"))]
    let dpi = window.scale_factor().map_err(|_| window_error())?;
    if let Some(bounds) = frame.screen_bounds.filter(|_| overlay) {
        #[cfg(target_os = "macos")]
        {
            layout.image_width = bounds.width;
            layout.image_height = bounds.height;
        }
        #[cfg(not(target_os = "macos"))]
        {
            layout.image_width = bounds.width / dpi;
            layout.image_height = bounds.height / dpi;
        }
    } else {
        let fit = (900.0 / frame.width as f64)
            .min(600.0 / frame.height as f64)
            .min(1.0);
        layout.image_width = frame.width as f64 * fit;
        layout.image_height = frame.height as f64 * fit;
    }
    if !same_frame {
        *lock(&app.state::<AppState>().result_toolbar) =
            crate::result_toolbar::ToolbarState::default();
        *lock(&app.state::<AppState>().result_display) = crate::result_toolbar::ResultDisplay {
            frame_id: frame.id.clone(),
            ..Default::default()
        };
    }
    layout.frame_id = frame.id.clone();
    *lock(&app.state::<AppState>().result_layout) = layout.clone();
    let _ = window.emit("result-layout-changed", layout);
    request_show(&window)
}

pub fn toggle_result(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("result")
        && window.is_visible().unwrap_or(false)
    {
        return crate::result_tools::hide(app);
    }
    let metadata = lock(&app.state::<AppState>().sessions).visual.frame.clone();
    if let Some(metadata) = metadata {
        show_result(app, &metadata)
    } else {
        show_main(app)
    }
}

fn retire_selections(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("selection-") {
            #[cfg(not(windows))]
            lock(&app.state::<AppState>().window_lifecycle).forget(&label);
            #[cfg(windows)]
            let _ = window.hide();
            #[cfg(not(windows))]
            let _ = window.destroy();
        }
    }
}

pub fn cancel_capture(app: &AppHandle, restore: bool) {
    let state = app.state::<AppState>();
    if restore
        && app
            .webview_windows()
            .keys()
            .any(|label| label.starts_with("selection-"))
    {
        state.jobs.cancel("visual");
        lock(&state.window_lifecycle).forget("result");
    }
    state.jobs.cancel("capture");
    retire_selections(app);
    let frames: Vec<_> = lock(&state.selections)
        .drain()
        .map(|(_, frame)| frame)
        .collect();
    for frame in frames {
        lock(&state.frames).remove(&frame);
    }
    if restore {
        let previous: Vec<_> = lock(&state.return_windows).drain(..).collect();
        for label in previous {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.show();
            }
        }
    } else {
        lock(&state.return_windows).clear();
    }
}

pub fn disable_selection_fade() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_SETSELECTIONFADE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };
        let _ = SystemParametersInfoW(
            SPI_SETSELECTIONFADE,
            0,
            Some(std::ptr::null_mut()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
}

#[cfg(windows)]
fn dismiss_transient_popups(from_tray: bool) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::{
        EndMenu, FindWindowExW, IsWindowVisible, SW_HIDE, ShowWindow,
    };
    use windows::core::w;

    let mut had_popup = false;
    unsafe {
        let _ = EndMenu();
        let classes: &[windows::core::PCWSTR] = if from_tray {
            &[
                w!("#32768"),
                w!("tooltips_class32"),
                w!("SysShadow"),
                w!("Xaml_WindowedPopupClass"),
            ]
        } else {
            &[w!("#32768")]
        };
        for &class_name in classes {
            let mut current = HWND::default();
            while let Ok(next) = FindWindowExW(HWND::default(), current, class_name, None) {
                if next.0.is_null() {
                    break;
                }
                current = next;
                if IsWindowVisible(next).as_bool() {
                    had_popup = true;
                    let disabled: i32 = 1;
                    let _ = DwmSetWindowAttribute(
                        next,
                        DWMWA_TRANSITIONS_FORCEDISABLED,
                        &disabled as *const _ as _,
                        std::mem::size_of::<i32>() as u32,
                    );
                    let _ = ShowWindow(next, SW_HIDE);
                }
            }
        }
    }
    had_popup
}

#[cfg(windows)]
fn hide_tray_overflow_if_still_open() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, IsWindowVisible, SW_HIDE, ShowWindow,
    };
    use windows::core::w;

    unsafe {
        for class_name in [
            w!("TopLevelWindowForOverflowXamlIsland"),
            w!("NotifyIconOverflowWindow"),
        ] {
            let mut current = HWND::default();
            while let Ok(next) = FindWindowExW(HWND::default(), current, class_name, None) {
                if next.0.is_null() {
                    break;
                }
                current = next;
                if IsWindowVisible(next).as_bool() {
                    let _ = ShowWindow(next, SW_HIDE);
                }
            }
        }
    }
}

#[cfg(windows)]
fn tray_overflow_visible() -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, IsWindowVisible};
    use windows::core::w;

    unsafe {
        for class_name in [
            w!("TopLevelWindowForOverflowXamlIsland"),
            w!("NotifyIconOverflowWindow"),
        ] {
            let mut current = HWND::default();
            while let Ok(next) = FindWindowExW(HWND::default(), current, class_name, None) {
                if next.0.is_null() {
                    break;
                }
                current = next;
                if IsWindowVisible(next).as_bool() {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(windows)]
async fn wait_for_tray_overflow_to_close() {
    // Only the overflow flyout needs time to finish its close animation.
    // Waiting only while that window exists keeps the normal tray path immediate.
    let deadline = Instant::now() + Duration::from_millis(160);
    while tray_overflow_visible() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(12)).await;
    }
    hide_tray_overflow_if_still_open();
    flush_compositor();
}

pub fn begin_capture(app: AppHandle, monitor: Option<String>) {
    begin_capture_with_source(app, monitor, false);
}

pub fn begin_capture_from_tray(app: AppHandle) {
    begin_capture_with_source(app, None, true);
}

fn hash_rgba(width: u32, height: u32, bytes: &[u8]) -> (u32, u32, u64) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    bytes.len().hash(&mut hasher);
    let step = (bytes.len() / 4096).max(1);
    for b in bytes.iter().step_by(step) {
        b.hash(&mut hasher);
    }
    (width, height, hasher.finish())
}

async fn clipboard_image_digest(app: AppHandle) -> Option<(u32, u32, u64)> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    tokio::task::spawn_blocking(move || {
        let image = app.clipboard().read_image().ok()?;
        let bytes = image.rgba();
        if image.width() == 0 || image.height() == 0 || bytes.is_empty() {
            return None;
        }
        Some(hash_rgba(image.width(), image.height(), bytes))
    })
    .await
    .ok()
    .flatten()
}

async fn recover_wayland_clipboard_capture(
    app: AppHandle,
    before: Option<(u32, u32, u64)>,
) -> Option<CaptureFrame> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    tokio::task::spawn_blocking(move || {
        for _ in 0..6 {
            if let Ok(image) = app.clipboard().read_image() {
                let width = image.width();
                let height = image.height();
                let bytes = image.rgba();
                if width > 0
                    && height > 0
                    && !bytes.is_empty()
                    && before != Some(hash_rgba(width, height, bytes))
                    && let Some(rgba) = image::RgbaImage::from_raw(width, height, bytes.to_vec())
                {
                    return Some(CaptureFrame {
                        image: rgba,
                        screen_bounds: None,
                        scale_factor: 1.0,
                    });
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    })
    .await
    .ok()
    .flatten()
}

pub fn begin_capture_with_source(app: AppHandle, monitor: Option<String>, from_tray: bool) {
    cancel_capture(&app, true);
    let state = app.state::<AppState>();
    let ticket = state.jobs.begin("capture");
    // Capture exactly what is visible. Hiding or capture-excluding the main
    // window changed the desktop at both ends of every capture in alpha.9.
    lock(&state.return_windows).clear();
    for label in ["quick", "result", "result-tools"] {
        if let Some(window) = app.get_webview_window(label)
            && window.is_visible().unwrap_or(false)
        {
            lock(&state.return_windows).push(label.into());
            disable_transitions(&window);
            let _ = window.hide();
        }
    }
    #[cfg(target_os = "linux")]
    if lock(&state.capabilities).wayland
        && let Some(window) = app.get_webview_window("main")
        && window.is_visible().unwrap_or(false)
    {
        lock(&state.return_windows).push("main".into());
        let _ = window.hide();
    }
    #[cfg(windows)]
    let overflow_visible = from_tray && tray_overflow_visible();
    #[cfg(windows)]
    let had_popup = dismiss_transient_popups(from_tray) || overflow_visible;
    #[cfg(windows)]
    if had_popup {
        flush_compositor();
    }
    #[cfg(not(windows))]
    {
        let _ = from_tray;
        flush_compositor();
    }
    tauri::async_runtime::spawn(async move {
        let result = async {
            #[cfg(windows)]
            if overflow_visible {
                wait_for_tray_overflow_to_close().await;
            }

            let state = app.state::<AppState>();
            let wayland = lock(&state.capabilities).wayland;
            if wayland {
                let before_clip = clipboard_image_digest(app.clone()).await;
                let frame = match SystemCapture.capture(None, ticket.stop.clone()).await {
                    Ok(frame) => frame,
                    Err(err) if err.code != ErrorCode::Cancelled => {
                        recover_wayland_clipboard_capture(app.clone(), before_clip)
                            .await
                            .ok_or(err)?
                    }
                    Err(err) => return Err(err),
                };
                if !state.jobs.is_current("capture", &ticket) {
                    return Err(AppError::cancelled());
                }
                let metadata = lock(&state.frames).insert(frame, &[])?;
                lock(&state.return_windows).clear();
                pipeline::start_visual(app.clone(), metadata, true);
                return Ok::<(), AppError>(());
            }
            let monitors = SystemCapture.monitors()?;
            *lock(&state.capture_layout) = monitors.clone();
            let monitors: Vec<_> = monitors
                .into_iter()
                .filter(|info| monitor.as_ref().is_none_or(|id| id == &info.id))
                .collect();
            if monitors.is_empty() {
                return Err(AppError::new(ErrorCode::InvalidInput, "没有可用的显示器"));
            }
            let mut captures: Vec<(usize, FrameMetadata)> = Vec::new();
            // Start every screen together, then await in monitor order so the
            // selected windows and frame cache remain deterministic.
            let capture_tasks: Vec<_> = monitors
                .into_iter()
                .enumerate()
                .map(|(index, info)| {
                    let stop = ticket.stop.clone();
                    tokio::spawn(async move {
                        let frame = SystemCapture.capture(Some(info.id), stop).await;
                        (index, frame)
                    })
                })
                .collect();
            for capture in capture_tasks {
                let (index, frame) = capture
                    .await
                    .map_err(|_| AppError::new(ErrorCode::Internal, "截屏任务异常结束，请重试"))?;
                let frame = frame?;
                if !state.jobs.is_current("capture", &ticket) {
                    return Err(AppError::cancelled());
                }
                let mut protected: Vec<_> = captures
                    .iter()
                    .map(|(_, meta): &(usize, FrameMetadata)| meta.id.clone())
                    .collect();
                if let Some(previous) = lock(&state.sessions).visual.frame.as_ref() {
                    protected.push(previous.id.clone());
                }
                let metadata = lock(&state.frames).insert(frame, &protected)?;
                captures.push((index, metadata));
            }
            {
                let mut selections = lock(&state.selections);
                for (index, metadata) in &captures {
                    selections.insert(format!("selection-{index}"), metadata.id.clone());
                }
            }
            for (index, metadata) in captures {
                let theme = lock(&app.state::<AppState>().config).appearance.theme;
                let theme_str = match theme {
                    screentrans_core::config::Theme::Dark => "dark",
                    screentrans_core::config::Theme::Light => "light",
                    screentrans_core::config::Theme::System => "system",
                };
                if !state.jobs.is_current("capture", &ticket) {
                    return Err(AppError::cancelled());
                }
                let label = format!("selection-{index}");
                let bounds = metadata.screen_bounds.ok_or_else(|| {
                    AppError::new(ErrorCode::Internal, "截图显示器边界无效，请重试")
                })?;
                lock(&state.selections).insert(label.clone(), metadata.id.clone());
                let window = if let Some(window) = app.get_webview_window(&label) {
                    lock(&state.window_lifecycle).forget(&label);
                    let _ = window.emit("selection-theme", theme_str);
                    let _ = window.emit("selection-frame", metadata.clone());
                    window
                } else {
                    selection_window(&app, &label)?
                };
                place_on_desktop(&window, bounds, &metadata)?;
                request_show(&window)?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = result
            && error.code != ErrorCode::Cancelled
            && app.state::<AppState>().jobs.is_current("capture", &ticket)
        {
            // A late OS/portal error from an abandoned capture must not tear
            // down selection windows that belong to its replacement.
            cancel_capture(&app, true);
            if app
                .get_webview_window("main")
                .is_some_and(|w| w.is_visible().unwrap_or(false))
            {
                let _ = show_main(&app);
            }
            let _ = app.emit("app-error", error);
        }
    });
}

pub fn accept_selection(app: &AppHandle, label: &str, rect: Rect) -> Result<()> {
    let state = app.state::<AppState>();
    let current = SystemCapture.monitors()?;
    let changed = {
        let previous = lock(&state.capture_layout);
        previous.len() != current.len() || previous.iter().any(|monitor| !current.contains(monitor))
    };
    if changed {
        cancel_capture(app, true);
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "显示器布局或缩放已变化，请重新截图",
        ));
    }
    let (crop, selected_frames): (CaptureFrame, Vec<String>) = {
        let mut selections = lock(&state.selections);
        let frame_id = selections
            .get(label)
            .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "截图选区已过期"))?;
        let crop = lock(&state.frames).crop(frame_id, rect)?;
        (crop, selections.drain().map(|(_, id)| id).collect())
    };
    // Keep the frozen selection windows visible until the decoded result is ready.
    for id in selected_frames {
        lock(&state.frames).remove(&id);
    }
    let metadata = lock(&state.frames).insert(crop, &[])?;
    pipeline::start_visual(app.clone(), metadata, true);
    Ok(())
}

pub fn close_requested(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    let app = window.app_handle();
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match window.label() {
        "main"
            if (cfg!(target_os = "macos") || lock(&state.config).appearance.close_to_tray)
                && lock(&state.capabilities).tray =>
        {
            api.prevent_close();
            let _ = window.hide();
            #[cfg(target_os = "macos")]
            screentrans_platform::hide_dock_icon();
        }
        "main" => {
            state.jobs.cancel_all();
            app.exit(0);
        }
        "result" | "result-tools" => {
            api.prevent_close();
            let _ = crate::result_tools::hide(app);
        }
        "quick" => {
            api.prevent_close();
            let _ = window.hide();
        }
        label if label.starts_with("selection-") => {
            api.prevent_close();
            cancel_capture(app, true);
        }
        _ => {}
    }
}

pub fn reap_hidden_windows(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut hidden: HashMap<String, Instant> = HashMap::new();
        let mut timer = tokio::time::interval(Duration::from_secs(15));
        loop {
            timer.tick().await;
            let state = app.state::<AppState>();
            for (label, window) in app.webview_windows() {
                if !matches!(label.as_str(), "result" | "result-tools") {
                    continue;
                }
                if window.is_visible().unwrap_or(true)
                    || lock(&state.dirty_windows).contains(&label)
                    || !lock(&state.return_windows).is_empty()
                {
                    hidden.remove(&label);
                    continue;
                }
                if hidden
                    .entry(label.clone())
                    .or_insert_with(Instant::now)
                    .elapsed()
                    >= Duration::from_secs(60)
                {
                    let _ = window.destroy();
                    lock(&state.window_lifecycle).forget(&label);
                    hidden.remove(&label);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_wait_for_content_and_cancelled_reveals_are_not_replayed() {
        let mut lifecycle = WindowLifecycle::default();
        assert!(!lifecycle.request("result"));
        assert!(lifecycle.loaded("result"));
        assert!(lifecycle.request("result"));
        lifecycle.forget("result");
        assert!(!lifecycle.loaded("result"));
        lifecycle.forget("result");
        assert!(!lifecycle.request("result"));
    }

    #[test]
    fn result_controls_do_not_move_or_resize_a_small_capture() {
        let image = Rect {
            x: -1500.0,
            y: 200.0,
            width: 120.0,
            height: 30.0,
        };
        let area = Rect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let (window, layout) = result_geometry(image, 1.5, Some(area));
        assert_eq!(window.width, 384.0);
        assert_eq!(
            window.height - (RESULT_CONTROLS_HEIGHT + 2.0 * RESULT_PADDING) * 1.5,
            image.height
        );
        assert_eq!(window.right() - RESULT_PADDING * 1.5 - image.width, image.x);
        assert_eq!(window.y + RESULT_PADDING * 1.5, image.y);
        assert!(!layout.controls_above);
        assert!(!layout.image_align_left);
    }

    #[test]
    fn controls_move_above_near_bottom_without_moving_the_image() {
        let image = Rect {
            x: 100.0,
            y: 950.0,
            width: 600.0,
            height: 80.0,
        };
        let area = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let (window, layout) = result_geometry(image, 1.0, Some(area));
        assert!(layout.controls_above);
        assert_eq!(window.y + RESULT_CONTROLS_HEIGHT + RESULT_PADDING, image.y);
        assert_eq!(window.bottom() - RESULT_PADDING, image.bottom());
    }

    #[test]
    fn narrow_capture_at_left_edge_keeps_toolbar_on_the_monitor() {
        let image = Rect {
            x: -1910.0,
            y: 100.0,
            width: 80.0,
            height: 10.0,
        };
        let area = Rect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let (window, layout) = result_geometry(image, 1.0, Some(area));
        assert!(layout.image_align_left);
        assert_eq!(window.x + RESULT_PADDING, image.x);
        assert_eq!(window.height, 74.0);
        assert!(window.x >= area.x);
        assert!(window.right() <= area.right());
    }
}
