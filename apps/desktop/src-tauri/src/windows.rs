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

fn caption_color(window: &tauri::WebviewWindow, theme: screentrans_core::config::Theme) {
    #[cfg(windows)]
    if let Ok(handle) = window.hwnd() {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DwmSetWindowAttribute,
        };
        let dark = match theme {
            screentrans_core::config::Theme::Dark => true,
            screentrans_core::config::Theme::Light => false,
            screentrans_core::config::Theme::System => system_dark(),
        };
        let (background, text): (u32, u32) = match theme {
            screentrans_core::config::Theme::Light => (0x00f5f5f5, 0x00242424),
            screentrans_core::config::Theme::Dark => (0x00121212, 0x00e5e5e5),
            screentrans_core::config::Theme::System => (0xffffffff, 0xffffffff),
        };
        unsafe {
            let hwnd = windows::Win32::Foundation::HWND(handle.0);
            if window.label() == "main" {
                crate::update_caption::refresh(
                    handle.0,
                    if dark { 0x121212 } else { 0xf5f5f5 },
                    if dark { 0xe5e5e5 } else { 0x242424 },
                );
            }
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
    let _ = (window, theme);
}

#[cfg(windows)]
fn system_dark() -> bool {
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

const RESULT_CONTROLS_HEIGHT: f64 = 48.0;
const RESULT_CONTROLS_WIDTH: f64 = 240.0;
const RESULT_PADDING: f64 = 8.0;

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
    let cursor = window.cursor_position().map_err(|_| window_error())?;
    let bounds = crate::result_tools::image_bounds(window)?;
    *lock(&state.result_resize) = Some(crate::result_resize::ResizeGesture {
        #[cfg(windows)]
        outline: crate::resize_outline::ResizeOutline::new(window, bounds, scale)?,
        frame_id: frame.id,
        bounds,
        current: bounds,
        cursor: (cursor.x, cursor.y),
        direction,
        scale,
        layout,
    });
    lock(&state.result_toolbar).image_resizing = true;
    crate::result_tools::sync(window.app_handle())?;
    Ok(())
}

pub fn update_result_resize(window: &tauri::WebviewWindow, finish: bool) -> Result<Option<Rect>> {
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
    let cursor = window.cursor_position().map_err(|_| window_error())?;
    let bounds = crate::result_resize::resize(
        gesture.bounds,
        gesture.direction,
        cursor.x - gesture.cursor.0,
        cursor.y - gesture.cursor.1,
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
            .set_position(tauri::LogicalPosition::new(outer.x, outer.y))
            .map_err(|_| window_error())?;
        window
            .set_size(tauri::LogicalSize::new(outer.width, outer.height))
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
        let _ = window.set_position(tauri::LogicalPosition::new(bounds.x, bounds.y));
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
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
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?window={query}").into()),
    )
    .title(match label {
        "result" => "ScreenTranslate · 截图",
        "result-tools" => "ScreenTranslate · 工具条",
        _ => "ScreenTranslate",
    })
    .theme(native_theme(
        lock(&app.state::<AppState>().config).appearance.theme,
    ))
    .visible(false)
    .on_navigation(|url| {
        url.scheme() == "tauri"
            || matches!(
                url.host_str(),
                Some("tauri.localhost" | "127.0.0.1" | "localhost")
            )
    })
}

pub fn show_main(app: &AppHandle) -> Result<()> {
    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None => builder(app, "main", "main")
            .inner_size(600.0, 400.0)
            .min_inner_size(600.0, 400.0)
            .center()
            .build()
            .map_err(|_| window_error())?,
    };
    let theme = lock(&app.state::<AppState>().config).appearance.theme;
    caption_color(&window, theme);
    request_show(&window)
}

pub fn toggle_quick(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("quick") {
        if window.is_visible().unwrap_or(false) {
            window.hide().map_err(|_| window_error())?;
        } else {
            request_show(&window)?;
            let _ = window.emit("focus-input", ());
        }
        return Ok(());
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
    request_show(&window)
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
            .resizable(false)
            .skip_taskbar(true)
            .build()
            .map_err(|_| window_error())?,
    };
    lock(&app.state::<AppState>().window_lifecycle).forget("result");
    let _ = window.hide();
    let _ = crate::result_tools::hide(app);
    let _ = crate::result_tools::prepare(app)?;
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
    let dpi = window.scale_factor().map_err(|_| window_error())?;
    if let Some(bounds) = frame.screen_bounds.filter(|_| overlay) {
        layout.image_width = bounds.width / dpi;
        layout.image_height = bounds.height / dpi;
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

fn close_selections(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("selection-") {
            lock(&app.state::<AppState>().window_lifecycle).forget(&label);
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
    close_selections(app);
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

pub fn begin_capture(app: AppHandle, monitor: Option<String>) {
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
    flush_compositor();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let state = app.state::<AppState>();
            let wayland = lock(&state.capabilities).wayland;
            if wayland {
                let frame = SystemCapture.capture(None, ticket.stop.clone()).await?;
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
            let mut captures = Vec::new();
            for info in monitors {
                let frame = SystemCapture
                    .capture(Some(info.id.clone()), ticket.stop.clone())
                    .await?;
                if !state.jobs.is_current("capture", &ticket) {
                    return Err(AppError::cancelled());
                }
                let mut protected: Vec<_> = captures
                    .iter()
                    .map(|meta: &FrameMetadata| meta.id.clone())
                    .collect();
                if let Some(previous) = lock(&state.sessions).visual.frame.as_ref() {
                    protected.push(previous.id.clone());
                }
                let metadata = lock(&state.frames).insert(frame, &protected)?;
                captures.push(metadata);
            }
            {
                let mut selections = lock(&state.selections);
                for metadata in &captures {
                    selections.insert(format!("selection-{}", metadata.id), metadata.id.clone());
                }
            }
            for metadata in captures {
                if !state.jobs.is_current("capture", &ticket) {
                    return Err(AppError::cancelled());
                }
                let label = format!("selection-{}", metadata.id);
                let bounds = metadata.screen_bounds.unwrap();
                lock(&state.selections).insert(label.clone(), metadata.id.clone());
                let window = builder(&app, &label, "selection")
                    .decorations(false)
                    .transparent(true)
                    .shadow(false)
                    .resizable(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .build()
                    .map_err(|_| window_error())?;
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
            let _ = show_main(&app);
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
            if lock(&state.config).appearance.close_to_tray && lock(&state.capabilities).tray =>
        {
            api.prevent_close();
            let _ = window.hide();
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
                if !matches!(label.as_str(), "main" | "quick" | "result" | "result-tools") {
                    continue;
                }
                if (label == "main"
                    && app
                        .state::<crate::update_ui::UpdateUi>()
                        .busy
                        .load(std::sync::atomic::Ordering::Relaxed))
                    || window.is_visible().unwrap_or(true)
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
