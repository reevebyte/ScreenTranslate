use crate::{
    state::{AppState, lock},
    windows,
};
use screentrans_core::{Rect, Result};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// Native Wayland windows cannot be positioned by the client, so the floating
/// toolbar stays embedded inside `result`. X11/XWayland, Windows and macOS use
/// the independent freely-draggable `result-tools` window.
pub(crate) fn uses_embedded_toolbar() -> bool {
    screentrans_platform::native_wayland_windows()
}

#[cfg(target_os = "linux")]
fn wayland_image_bounds(layout: &windows::ResultLayout, scale: f64) -> Rect {
    let width = layout.image_width * scale;
    let padding = windows::RESULT_PADDING * scale;
    let viewport_width = layout.image_width.max(windows::RESULT_CONTROLS_WIDTH) * scale;
    Rect {
        x: if layout.image_align_left {
            padding
        } else {
            viewport_width + padding - width
        },
        y: windows::RESULT_PADDING * scale
            + if layout.controls_above {
                windows::RESULT_CONTROLS_HEIGHT * scale
            } else {
                0.0
            },
        width,
        height: layout.image_height * scale,
    }
}

#[cfg(any(target_os = "linux", test))]
fn resized_layout_for_wayland(
    layout: &windows::ResultLayout,
    direction: crate::result_resize::ResizeDirection,
    scale: f64,
    start_size: (u32, u32),
    final_size: (u32, u32),
) -> windows::ResultLayout {
    use crate::result_resize::ResizeDirection::*;
    let mut resized = layout.clone();
    if matches!(
        direction,
        NorthWest | NorthEast | East | SouthEast | SouthWest | West
    ) {
        let start = layout.image_width;
        let delta = (final_size.0 as f64 - start_size.0 as f64) / scale;
        resized.image_width = (start + delta).clamp(32.0_f64.min(start), 16000.0);
    }
    if matches!(
        direction,
        NorthWest | NorthEast | North | SouthEast | South | SouthWest
    ) {
        let start = layout.image_height;
        let delta = (final_size.1 as f64 - start_size.1 as f64) / scale;
        resized.image_height = (start + delta).clamp(12.0_f64.min(start), 16000.0);
    }
    resized
}

pub fn image_bounds(window: &WebviewWindow) -> Result<Rect> {
    let state = window.state::<AppState>();
    let frame = lock(&state.sessions)
        .visual
        .frame
        .clone()
        .ok_or_else(windows::window_error)?;
    let layout = lock(&state.result_layout).clone();
    if layout.frame_id != frame.id {
        return Err(windows::window_error());
    }
    let scale = window.scale_factor().map_err(|_| windows::window_error())?;
    #[cfg(target_os = "linux")]
    if uses_embedded_toolbar() && layout.image_width > 0.0 && layout.image_height > 0.0 {
        return Ok(wayland_image_bounds(&layout, scale));
    }
    let pos = window
        .inner_position()
        .map_err(|_| windows::window_error())?;
    let size = window.inner_size().map_err(|_| windows::window_error())?;
    // Match the CSS viewport directly, including the reserved width of tiny captures.
    let spare = (240.0 - layout.image_width).max(0.0) * scale;
    let width = (size.width as f64 - 16.0 * scale - spare).max(1.0);
    let height = (size.height as f64 - 64.0 * scale).max(1.0);
    Ok(Rect {
        x: pos.x as f64
            + if layout.image_align_left {
                8.0 * scale
            } else {
                size.width as f64 - 8.0 * scale - width
            },
        y: pos.y as f64 + (8.0 + if layout.controls_above { 48.0 } else { 0.0 }) * scale,
        width,
        height,
    })
}
fn monitor(window: &WebviewWindow, bounds: Rect) -> Result<(Rect, f64)> {
    let monitors = window
        .available_monitors()
        .map_err(|_| windows::window_error())?;
    let m = monitors
        .into_iter()
        .max_by(|a, b| {
            let overlap = |m: &tauri::Monitor| {
                let p = m.position();
                let s = m.size();
                (bounds.right().min(p.x as f64 + s.width as f64) - bounds.x.max(p.x as f64))
                    .max(0.0)
                    * (bounds.bottom().min(p.y as f64 + s.height as f64) - bounds.y.max(p.y as f64))
                        .max(0.0)
            };
            overlap(a).total_cmp(&overlap(b))
        })
        .ok_or_else(windows::window_error)?;
    let a = m.work_area();
    Ok((
        Rect {
            x: a.position.x as f64,
            y: a.position.y as f64,
            width: a.size.width as f64,
            height: a.size.height as f64,
        },
        m.scale_factor(),
    ))
}
#[cfg(target_os = "macos")]
fn detach_macos_child(app: &AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let (Some(image), Some(tool)) = (
            app_handle.get_webview_window("result"),
            app_handle.get_webview_window("result-tools"),
        ) && let (Ok(image_ptr), Ok(tool_ptr)) = (image.ns_window(), tool.ns_window())
        {
            unsafe {
                screentrans_platform::detach_child_window(image_ptr, tool_ptr);
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn attach_macos_child(app: &AppHandle, _image: &WebviewWindow, _tool: &WebviewWindow) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let (Some(image), Some(tool)) = (
            app_handle.get_webview_window("result"),
            app_handle.get_webview_window("result-tools"),
        ) && let (Ok(image_ptr), Ok(tool_ptr)) = (image.ns_window(), tool.ns_window())
        {
            unsafe {
                screentrans_platform::attach_child_window(image_ptr, tool_ptr);
            }
        }
    });
}

struct PlacingGuard(AppHandle);
impl PlacingGuard {
    fn enter(app: &AppHandle, target_x: i32, target_y: i32) -> Self {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.placing += 1;
        bar.expected = Some((target_x, target_y));
        Self(app.clone())
    }
}
impl Drop for PlacingGuard {
    fn drop(&mut self) {
        let state = self.0.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.placing = bar.placing.saturating_sub(1);
    }
}

pub fn prepare(app: &AppHandle) -> Result<WebviewWindow> {
    if let Some(tool) = app.get_webview_window("result-tools") {
        return Ok(tool);
    }
    let builder = windows::builder(app, "result-tools", "result-tools");
    #[cfg(windows)]
    let builder = if let Some(image) = app.get_webview_window("result") {
        builder.owner(&image).map_err(|_| windows::window_error())?
    } else {
        builder
    };
    let tool = builder
        .inner_size(
            crate::result_toolbar::TOOLBAR_WIDTH,
            crate::result_toolbar::TOOLBAR_HEIGHT,
        )
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .accept_first_mouse(true)
        .build()
        .map_err(|_| windows::window_error())?;
    #[cfg(target_os = "linux")]
    {
        windows::tune_linux_webview(&tool);
        let tool_clone = tool.clone();
        let _ = tool.run_on_main_thread(move || {
            if let Ok(gtk_win) = tool_clone.gtk_window() {
                use gtk::prelude::GtkWindowExt;
                // Utility hint lets GNOME Mutter / XWayland honor exact docked
                // placement over the transparent bottom strip of `result` while
                // keeping the floating toolbar freely draggable.
                gtk_win.set_type_hint(gtk::gdk::WindowTypeHint::Utility);
                gtk_win.set_gravity(gtk::gdk::Gravity::NorthWest);
            }
        });
    }
    #[cfg(target_os = "macos")]
    {
        let tool_clone = tool.clone();
        let _ = tool.run_on_main_thread(move || {
            if let Ok(ns_window) = tool_clone.ns_window() {
                unsafe {
                    screentrans_platform::configure_transparent_window(ns_window);
                }
            }
        });
    }
    Ok(tool)
}
fn place_with(
    app: &AppHandle,
    tool: &WebviewWindow,
    bounds: Rect,
    #[cfg_attr(windows, allow(unused_variables))] scale: f64,
    force: bool,
) -> Result<()> {
    let target_x = bounds.x.round() as i32;
    let target_y = bounds.y.round() as i32;
    let target_w = bounds.width.round() as u32;
    let target_h = bounds.height.round() as u32;
    let guard = PlacingGuard::enter(app, target_x, target_y);
    let pos = tool.inner_position().map_err(|_| windows::window_error())?;
    let size = tool.inner_size().map_err(|_| windows::window_error())?;
    let pos_diff = force || (pos.x - target_x).abs() > 1 || (pos.y - target_y).abs() > 1;
    let size_diff = size.width.abs_diff(target_w) > 1 || size.height.abs_diff(target_h) > 1;
    if pos_diff || size_diff {
        #[cfg(target_os = "macos")]
        {
            let scale = scale.max(1.0);
            if size_diff {
                tool.set_size(tauri::LogicalSize::new(
                    bounds.width / scale,
                    bounds.height / scale,
                ))
                .map_err(|_| windows::window_error())?;
            }
            tool.set_position(tauri::LogicalPosition::new(
                bounds.x / scale,
                bounds.y / scale,
            ))
            .map_err(|_| windows::window_error())?;
        }
        #[cfg(target_os = "linux")]
        {
            // Pass GTK logical coordinates using the image/monitor scale directly so a
            // newly-created hidden toolbar window with a stale scale factor cannot be
            // placed at 2x coordinates on HiDPI XWayland displays.
            let scale = scale.max(1.0);
            let lw = (bounds.width / scale).round().max(1.0) as i32;
            let lh = (bounds.height / scale).round().max(1.0) as i32;
            let lx = (bounds.x / scale).round() as i32;
            let ly = (bounds.y / scale).round() as i32;
            if size_diff || force {
                tool.set_size(tauri::LogicalSize::new(lw as f64, lh as f64))
                    .map_err(|_| windows::window_error())?;
            }
            tool.set_position(tauri::LogicalPosition::new(lx as f64, ly as f64))
                .map_err(|_| windows::window_error())?;
            if force || size_diff {
                // GTK3's `gtk_window_move` short-circuits when asked for the same
                // pre-map coordinates even after Mutter placed the window elsewhere.
                // Moving the realized GDK window directly sends an unconditional
                // X11 ConfigureRequest so the toolbar snaps tightly below the capture.
                // Keep `guard` alive inside the main-thread closure so `placing > 0`
                // covers the asynchronous GTK move itself.
                let tool_clone = tool.clone();
                let _ = tool.run_on_main_thread(move || {
                    let _keep = guard;
                    if let Ok(gtk_win) = tool_clone.gtk_window() {
                        use gtk::prelude::{GtkWindowExt, WidgetExt};
                        if size_diff {
                            gtk_win.set_size_request(lw, lh);
                            gtk_win.resize(lw, lh);
                        }
                        gtk_win.move_(lx, ly);
                        if let Some(gdk_win) = gtk_win.window() {
                            if size_diff {
                                gdk_win.move_resize(lx, ly, lw, lh);
                            } else {
                                gdk_win.move_(lx, ly);
                            }
                        }
                    }
                });
                return Ok(());
            }
        }
        #[cfg(windows)]
        {
            windows::set_client_rect(tool, bounds)?;
        }
        // X11/XWayland moves are asynchronous, so reading `inner_position()`
        // right away can return the pre-move coordinate and overwrite `expected`.
        #[cfg(not(target_os = "linux"))]
        if let Ok(actual) = tool.inner_position() {
            lock(&app.state::<AppState>().result_toolbar).expected = Some((actual.x, actual.y));
        }
    }
    drop(guard);
    Ok(())
}
pub fn clamp_toolbar(app: &AppHandle) -> Result<()> {
    let Some(tool) = app.get_webview_window("result-tools") else {
        return Ok(());
    };
    let menu_open = lock(&app.state::<AppState>().result_toolbar).menu_open;
    let p = tool.inner_position().map_err(|_| windows::window_error())?;
    let s = tool.inner_size().map_err(|_| windows::window_error())?;
    let current = Rect {
        x: p.x as f64,
        y: p.y as f64,
        width: s.width as f64,
        height: s.height as f64,
    };
    let (area, scale) = monitor(&tool, current)?;
    let bounds = if menu_open {
        current
    } else {
        Rect {
            width: crate::result_toolbar::TOOLBAR_WIDTH * scale,
            height: crate::result_toolbar::TOOLBAR_HEIGHT * scale,
            ..current
        }
    };
    place_with(
        app,
        &tool,
        crate::result_toolbar::clamp(bounds, area),
        scale,
        false,
    )
}
pub fn sync(app: &AppHandle) -> Result<()> {
    if uses_embedded_toolbar() {
        return Ok(());
    }
    let Some(image) = app.get_webview_window("result") else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let settings = lock(&state.result_toolbar).clone();
    if settings.toolbar_moving {
        return Ok(());
    }
    if !image.is_visible().unwrap_or(false) || settings.suppressed() {
        #[cfg(target_os = "macos")]
        detach_macos_child(app);
        if let Some(tool) = app.get_webview_window("result-tools") {
            let _ = tool.hide();
        }
        return Ok(());
    }
    let tool = prepare(app)?;
    if settings.detached {
        #[cfg(target_os = "macos")]
        detach_macos_child(app);
        clamp_toolbar(app)?;
    } else {
        #[cfg(target_os = "macos")]
        detach_macos_child(app);
        let bounds = image_bounds(&image)?;
        let (area, scale) = monitor(&image, bounds)?;
        let base = crate::result_toolbar::placement(bounds, scale, &settings, area);
        let (rect, menu_above) = if settings.menu_open {
            crate::result_toolbar::menu_rect(base, scale, area)
        } else {
            (base, false)
        };
        place_with(app, &tool, rect, scale, false)?;
        {
            let mut bar = lock(&state.result_toolbar);
            bar.above = base.bottom() <= bounds.y;
            bar.menu_above = menu_above;
            #[cfg(target_os = "linux")]
            {
                bar.realign_attempts = 4;
            }
        }
        #[cfg(target_os = "macos")]
        attach_macos_child(app, &image, &tool);
    }
    let placement_state = lock(&state.result_toolbar).clone();
    let _ = tool.emit("toolbar-placement-changed", placement_state);
    if !tool.is_visible().unwrap_or(false) {
        windows::request_show(&tool)?;
    }
    Ok(())
}

pub fn set_menu_open(app: &AppHandle, open: bool) -> Result<bool> {
    let state = app.state::<AppState>();
    if uses_embedded_toolbar() {
        let mut bar = lock(&state.result_toolbar);
        bar.menu_open = open;
        bar.menu_above = false;
        return Ok(false);
    }
    let Some(tool) = app.get_webview_window("result-tools") else {
        return Ok(false);
    };
    let settings = {
        let mut bar = lock(&state.result_toolbar);
        if bar.toolbar_moving || bar.suppressed() {
            bar.menu_open = false;
            bar.menu_above = false;
            return Ok(false);
        }
        let prev = bar.clone();
        bar.menu_open = open;
        prev
    };
    let (base, area, scale, image_top) = if !settings.detached
        && let Some(image) = app.get_webview_window("result")
        && let Ok(bounds) = image_bounds(&image)
        && let Ok((area, scale)) = monitor(&image, bounds)
    {
        (
            crate::result_toolbar::placement(bounds, scale, &settings, area),
            area,
            scale,
            Some(bounds.y),
        )
    } else {
        let pos = tool.inner_position().map_err(|_| windows::window_error())?;
        let size = tool.inner_size().map_err(|_| windows::window_error())?;
        let current = Rect {
            x: pos.x as f64,
            y: pos.y as f64,
            width: size.width as f64,
            height: size.height as f64,
        };
        let (area, scale) = monitor(&tool, current)?;
        let pill_height = crate::result_toolbar::TOOLBAR_HEIGHT * scale;
        let extra = crate::result_toolbar::TOOLBAR_MENU_EXTRA * scale;
        let base_y = if settings.menu_open && settings.menu_above {
            current.y + extra
        } else {
            current.y
        };
        (
            crate::result_toolbar::clamp(
                Rect {
                    x: current.x,
                    y: base_y,
                    width: crate::result_toolbar::TOOLBAR_WIDTH * scale,
                    height: pill_height,
                },
                area,
            ),
            area,
            scale,
            None,
        )
    };
    let (target, menu_above) = if open {
        crate::result_toolbar::menu_rect(base, scale, area)
    } else {
        (base, false)
    };
    place_with(app, &tool, target, scale, true)?;
    let placement_state = {
        let mut bar = lock(&state.result_toolbar);
        if let Some(top) = image_top {
            bar.above = base.bottom() <= top;
        }
        bar.menu_open = open;
        bar.menu_above = menu_above;
        bar.clone()
    };
    let _ = tool.emit("toolbar-placement-changed", placement_state);
    Ok(menu_above)
}

#[cfg(target_os = "linux")]
fn tool_matches_rect_logical(tool: &WebviewWindow, rect: Rect, scale: f64) -> bool {
    let Ok(pos) = tool.inner_position() else {
        return false;
    };
    let tool_scale = tool.scale_factor().unwrap_or(scale).max(1.0);
    let scale = scale.max(1.0);
    (pos.x as f64 / tool_scale - rect.x / scale).abs() <= 2.0
        && (pos.y as f64 / tool_scale - rect.y / scale).abs() <= 2.0
}

#[cfg(target_os = "linux")]
fn realign_docked_toolbar(app: &AppHandle, force: bool) -> bool {
    let settings = {
        let state = app.state::<AppState>();
        let bar = lock(&state.result_toolbar);
        if bar.detached || bar.toolbar_moving || bar.suppressed() {
            return false;
        }
        bar.clone()
    };
    let (Some(image), Some(tool)) = (
        app.get_webview_window("result"),
        app.get_webview_window("result-tools"),
    ) else {
        return false;
    };
    if !image.is_visible().unwrap_or(false) || !tool.is_visible().unwrap_or(false) {
        return false;
    }
    let Ok(bounds) = image_bounds(&image) else {
        return false;
    };
    let Ok((area, scale)) = monitor(&image, bounds) else {
        return false;
    };
    let base = crate::result_toolbar::placement(bounds, scale, &settings, area);
    let (rect, menu_above) = if settings.menu_open {
        crate::result_toolbar::menu_rect(base, scale, area)
    } else {
        (base, false)
    };
    if !force && tool_matches_rect_logical(&tool, rect, scale) {
        return true;
    }
    let _ = place_with(app, &tool, rect, scale, true);
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.above = base.bottom() <= bounds.y;
        bar.menu_above = menu_above;
    }
    false
}

/// X11 / XWayland window managers (notably GNOME Mutter) often override a
/// window's pre-map position when `show()` maps it. Re-apply the docked
/// rectangle after the window maps and settles without holding any state lock
/// across GTK window queries (which would deadlock the main event loop).
#[cfg(target_os = "linux")]
pub(crate) fn realign_after_show(app: &AppHandle) {
    if uses_embedded_toolbar() {
        return;
    }
    let generation = {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        if bar.detached || bar.toolbar_moving || bar.suppressed() {
            return;
        }
        bar.realign_attempts = 4;
        bar.generation
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for (step, delay_ms) in [30_u64, 70, 110, 160].into_iter().enumerate() {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let active = {
                let state = app.state::<AppState>();
                let bar = lock(&state.result_toolbar);
                !bar.detached
                    && !bar.toolbar_moving
                    && !bar.suppressed()
                    && bar.generation == generation
            };
            if !active {
                break;
            }
            let settled = realign_docked_toolbar(&app, step == 0);
            if settled && step >= 1 {
                break;
            }
        }
    });
}
pub fn dock(app: &AppHandle, above: bool, align_left: bool) -> Result<windows::ResultLayout> {
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.above = above;
        bar.preferred_above = above;
        bar.align_left = align_left;
        bar.detached = false;
        bar.toolbar_moving = false;
        bar.menu_open = false;
        bar.menu_above = false;
        bar.generation += 1;
        bar.realign_attempts = 4;
    }
    let state = app.state::<AppState>();
    let mut result = lock(&state.result_layout).clone();
    let bar = lock(&state.result_toolbar).clone();
    result.controls_above = bar.above;
    result.toolbar_align_left = bar.align_left;
    if uses_embedded_toolbar() {
        *lock(&state.result_layout) = result.clone();
        let _ = app.emit_to("result", "result-layout-changed", result.clone());
    } else {
        sync(app)?;
    }
    Ok(result)
}
pub fn begin_drag(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    let generation = {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.detached = true;
        bar.toolbar_moving = true;
        bar.menu_open = false;
        bar.menu_above = false;
        bar.expected = None;
        bar.realign_attempts = 0;
        bar.generation += 1;
        bar.gesture_generation += 1;
        bar.gesture_generation
    };
    #[cfg(not(target_os = "linux"))]
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.detached = true;
        bar.toolbar_moving = true;
        bar.menu_open = false;
        bar.menu_above = false;
        bar.expected = None;
        bar.realign_attempts = 0;
        bar.generation += 1;
    }
    #[cfg(target_os = "linux")]
    if !uses_embedded_toolbar() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(8)).await;
            finish_linux_toolbar_drag(&app, Some(generation));
        });
    }
    #[cfg(target_os = "macos")]
    detach_macos_child(app);
}
pub async fn finish_drag(app: &AppHandle) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        finish_linux_toolbar_drag(app, None);
        Ok(())
    }
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        while unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0 {
            if !lock(&app.state::<AppState>().result_toolbar).toolbar_moving {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    }
    #[cfg(target_os = "macos")]
    {
        while screentrans_platform::mouse_button_down() {
            if !lock(&app.state::<AppState>().result_toolbar).toolbar_moving {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        {
            let state = app.state::<AppState>();
            let mut bar = lock(&state.result_toolbar);
            if !bar.toolbar_moving {
                return Ok(());
            }
            bar.toolbar_moving = false;
        }
        clamp_toolbar(app)
    }
}
pub fn begin_image_drag(app: &AppHandle) -> Result<()> {
    #[cfg(target_os = "linux")]
    let generation = {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.image_moving = true;
        bar.gesture_generation += 1;
        bar.gesture_generation
    };
    #[cfg(not(target_os = "linux"))]
    {
        lock(&app.state::<AppState>().result_toolbar).image_moving = true;
    }
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(8)).await;
            finish_wayland_drag_gesture(&app, Some(generation));
        });
    }
    sync(app)
}
pub fn finish_image_gesture(app: &AppHandle) -> Result<()> {
    lock(&app.state::<AppState>().result_toolbar).finish_image_gesture();
    sync(app)
}

#[cfg(target_os = "linux")]
fn finish_linux_toolbar_drag(app: &AppHandle, expected_generation: Option<u64>) {
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        if expected_generation.is_some_and(|expected| expected != bar.gesture_generation)
            || !bar.toolbar_moving
        {
            return;
        }
        bar.toolbar_moving = false;
    }
    let _ = clamp_toolbar(app);
    let _ = app.emit_to("result-tools", "toolbar-drag-ended", ());
}

#[cfg(target_os = "linux")]
fn schedule_linux_toolbar_drag_finish(window: &tauri::Window) {
    let app = window.app_handle().clone();
    let generation = {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        if !bar.toolbar_moving {
            return;
        }
        bar.gesture_generation += 1;
        bar.gesture_generation
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(240)).await;
        finish_linux_toolbar_drag(&app, Some(generation));
    });
}

#[cfg(target_os = "linux")]
fn finish_wayland_drag_gesture(app: &AppHandle, expected_generation: Option<u64>) {
    let state = app.state::<AppState>();
    let frame_id = {
        let mut bar = lock(&state.result_toolbar);
        if expected_generation.is_some_and(|expected| expected != bar.gesture_generation)
            || !bar.image_moving
        {
            return;
        }
        bar.finish_image_gesture();
        lock(&state.result_frame_id).clone()
    };
    let _ = sync(app);
    let _ = app.emit_to("result", "result-drag-ended", frame_id);
}

#[cfg(target_os = "linux")]
fn finish_wayland_resize_gesture(
    app: &AppHandle,
    window: &WebviewWindow,
    expected_generation: u64,
) {
    let state = app.state::<AppState>();
    if lock(&state.result_toolbar).gesture_generation != expected_generation {
        return;
    }
    let Some(gesture) = lock(&state.result_resize).clone() else {
        return;
    };
    if *lock(&state.result_frame_id) != gesture.frame_id {
        *lock(&state.result_resize) = None;
        return;
    }
    let Ok(size) = window.inner_size() else {
        return;
    };
    let layout = resized_layout_for_wayland(
        &gesture.layout,
        gesture.direction,
        gesture.scale,
        gesture.start_size,
        (size.width, size.height),
    );
    *lock(&state.result_layout) = layout.clone();
    *lock(&state.result_resize) = None;
    let _ = finish_image_gesture(app);
    let _ = window.emit("result-layout-changed", layout);
    let _ = app.emit_to("result", "result-resize-ended", gesture.frame_id);
}

#[cfg(target_os = "linux")]
fn schedule_wayland_drag_finish(window: &tauri::Window) {
    let app = window.app_handle().clone();
    let generation = {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        if !bar.image_moving {
            return;
        }
        bar.gesture_generation += 1;
        bar.gesture_generation
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(240)).await;
        finish_wayland_drag_gesture(&app, Some(generation));
    });
}

#[cfg(target_os = "linux")]
pub(crate) fn schedule_linux_resize_fallback(app: &AppHandle, generation: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        let Some(window) = app.get_webview_window("result") else {
            return;
        };
        finish_wayland_resize_gesture(&app, &window, generation);
    });
}

#[cfg(target_os = "linux")]
fn schedule_wayland_resize_finish(window: &tauri::Window) {
    let app = window.app_handle().clone();
    let generation = {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        if !bar.image_resizing {
            return;
        }
        bar.gesture_generation += 1;
        bar.gesture_generation
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(180)).await;
        let Some(window) = app.get_webview_window("result") else {
            return;
        };
        finish_wayland_resize_gesture(&app, &window, generation);
    });
}

pub async fn finish_image_drag(window: &WebviewWindow) -> Result<()> {
    let frame_id = lock(&window.state::<AppState>().result_frame_id).clone();
    #[cfg(target_os = "linux")]
    {
        if *lock(&window.state::<AppState>().result_frame_id) == frame_id {
            finish_wayland_drag_gesture(window.app_handle(), None);
        }
        Ok(())
    }
    // On Windows startDragging dispatches the native move loop before its Promise
    // resolves. Wait for the actual mouse release rather than that IPC reply.
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        while unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0 {
            if !lock(&window.state::<AppState>().result_toolbar).image_moving {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    }
    #[cfg(target_os = "macos")]
    {
        while screentrans_platform::mouse_button_down() {
            if !lock(&window.state::<AppState>().result_toolbar).image_moving {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if *lock(&window.state::<AppState>().result_frame_id) == frame_id {
            finish_image_gesture(window.app_handle())?;
        }
        Ok(())
    }
}
pub fn quiet(app: &AppHandle, quiet: bool) -> Result<()> {
    lock(&app.state::<AppState>().result_toolbar).quiet = quiet;
    sync(app)
}
#[cfg(target_os = "linux")]
pub fn cancel_toolbar_drag(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut bar = lock(&state.result_toolbar);
        bar.toolbar_moving = false;
        bar.detached = false;
        bar.gesture_generation += 1;
    }
    let _ = sync(app);
}

pub fn hide(app: &AppHandle) -> Result<()> {
    // Renderer cleanup is not guaranteed after hide/crash; release native resize
    // surfaces here as well as on normal pointer release and owner destruction.
    *lock(&app.state::<AppState>().result_resize) = None;
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.finish_image_gesture();
        bar.placing = 0;
    }
    #[cfg(target_os = "macos")]
    detach_macos_child(app);
    for label in ["result", "result-tools"] {
        if let Some(w) = app.get_webview_window(label) {
            w.hide().map_err(|_| windows::window_error())?;
        }
    }
    Ok(())
}
pub fn window_changed(window: &tauri::Window) {
    let label = window.label().to_owned();
    if label != "result" && label != "result-tools" {
        return;
    }
    if uses_embedded_toolbar() {
        #[cfg(target_os = "linux")]
        if label == "result" {
            schedule_wayland_drag_finish(window);
        }
        return;
    }
    {
        let state = window.state::<AppState>();
        let bar = lock(&state.result_toolbar);
        if label == "result" && bar.image_moving {
            drop(bar);
            #[cfg(target_os = "linux")]
            schedule_wayland_drag_finish(window);
            return;
        }
        if label == "result" && bar.image_resizing {
            return;
        }
        if label == "result-tools" && bar.toolbar_moving {
            drop(bar);
            #[cfg(target_os = "linux")]
            schedule_linux_toolbar_drag_finish(window);
            return;
        }
        if label == "result-tools" && (bar.is_placing() || bar.image_moving || bar.image_resizing) {
            return;
        }
        // On Linux X11/XWayland, Mutter can emit a late Moved event when
        // placing a newly mapped `result-tools` window far from the capture.
        // Snap it back with a bounded attempt budget so ping-pong is impossible
        // and user drags (`detached` / `toolbar_moving`) are never interfered with.
        #[cfg(target_os = "linux")]
        if label == "result-tools" {
            drop(bar);
            let retry_generation = {
                let mut bar = lock(&state.result_toolbar);
                if bar.detached
                    || bar.toolbar_moving
                    || bar.is_placing()
                    || bar.suppressed()
                    || bar.realign_attempts == 0
                {
                    None
                } else {
                    bar.realign_attempts -= 1;
                    Some(bar.generation)
                }
            };
            if let Some(generation) = retry_generation {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    let active = {
                        let state = app.state::<AppState>();
                        let bar = lock(&state.result_toolbar);
                        bar.generation == generation
                            && !bar.detached
                            && !bar.toolbar_moving
                            && !bar.is_placing()
                            && !bar.suppressed()
                    };
                    if active {
                        let _ = realign_docked_toolbar(&app, false);
                    }
                });
            }
            return;
        }
    }
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        // Window API work stays outside the native event callback.
        if label == "result-tools" {
            let Some(tool) = app.get_webview_window("result-tools") else {
                return;
            };
            if !tool.is_visible().unwrap_or(false) {
                return;
            }
            let Ok(pos) = tool.inner_position() else {
                return;
            };
            let state = app.state::<AppState>();
            #[cfg(not(target_os = "linux"))]
            let mut bar = lock(&state.result_toolbar);
            #[cfg(target_os = "linux")]
            let bar = lock(&state.result_toolbar);
            if bar.toolbar_moving || bar.is_placing() || bar.matches_expected(pos.x, pos.y) {
                return;
            }
            // On Linux X11/XWayland, Mutter emits Moved when applying initial
            // placement; only `begin_drag` detaches the toolbar there.
            #[cfg(not(target_os = "linux"))]
            {
                bar.detached = true;
            }
        }
        let generation = {
            let state = app.state::<AppState>();
            let mut bar = lock(&state.result_toolbar);
            if bar.toolbar_moving || bar.is_placing() {
                return;
            }
            bar.generation += 1;
            bar.generation
        };
        tokio::time::sleep(Duration::from_millis(if label == "result" {
            20
        } else {
            120
        }))
        .await;
        let should_sync = {
            let state = app.state::<AppState>();
            let bar = lock(&state.result_toolbar);
            bar.generation == generation && !bar.toolbar_moving && !bar.is_placing()
        };
        if should_sync {
            let _ = sync(&app);
        }
    });
}

pub fn window_resized(
    window: &tauri::Window,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] size: tauri::PhysicalSize<u32>,
) {
    let label = window.label();
    if label != "result" && label != "result-tools" {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if label == "result" {
            let state = window.state::<AppState>();
            let gesture = lock(&state.result_resize).clone();
            if let Some(gesture) = gesture {
                let current_frame = lock(&state.result_frame_id).clone();
                if current_frame == gesture.frame_id {
                    let layout = resized_layout_for_wayland(
                        &gesture.layout,
                        gesture.direction,
                        gesture.scale,
                        gesture.start_size,
                        (size.width, size.height),
                    );
                    *lock(&state.result_layout) = layout.clone();
                    let _ = window
                        .app_handle()
                        .emit_to("result", "result-layout-changed", layout);
                    schedule_wayland_resize_finish(window);
                    return;
                }
            }
        }
        if uses_embedded_toolbar() {
            return;
        }
    }
    window_changed(window);
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::wayland_image_bounds;
    use crate::windows::ResultLayout;

    #[test]
    fn wayland_small_capture_bounds_follow_css_alignment() {
        let layout = ResultLayout {
            frame_id: "frame".into(),
            image_width: 24.0,
            image_height: 12.0,
            ..Default::default()
        };
        let left = wayland_image_bounds(
            &ResultLayout {
                image_align_left: true,
                ..layout.clone()
            },
            2.0,
        );
        let right = wayland_image_bounds(&layout, 2.0);
        assert_eq!((left.x, left.width), (16.0, 48.0));
        assert_eq!((right.x, right.width), (448.0, 48.0));
    }
}

#[cfg(test)]
mod resize_tests {
    use super::resized_layout_for_wayland;
    use crate::{result_resize::ResizeDirection, windows::ResultLayout};

    #[test]
    fn wayland_native_resize_keeps_the_unused_viewport_reserve() {
        let layout = ResultLayout {
            frame_id: "frame".into(),
            image_width: 40.0,
            image_height: 20.0,
            ..Default::default()
        };
        let resized = resized_layout_for_wayland(
            &layout,
            ResizeDirection::SouthEast,
            1.0,
            (256, 84),
            (286, 124),
        );
        assert_eq!((resized.image_width, resized.image_height), (70.0, 60.0));
    }

    #[test]
    fn wayland_native_resize_changes_only_the_dragged_axes() {
        let layout = ResultLayout {
            image_width: 400.0,
            image_height: 200.0,
            ..Default::default()
        };
        let north = resized_layout_for_wayland(
            &layout,
            ResizeDirection::North,
            1.5,
            (616, 364),
            (616, 304),
        );
        assert_eq!((north.image_width, north.image_height), (400.0, 160.0));
        let west =
            resized_layout_for_wayland(&layout, ResizeDirection::West, 1.5, (616, 364), (556, 364));
        assert_eq!((west.image_width, west.image_height), (360.0, 200.0));
    }
}
