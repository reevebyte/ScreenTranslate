use crate::{
    state::{AppState, lock},
    windows,
};
use screentrans_core::{Rect, Result};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

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
    if screentrans_platform::native_wayland_windows()
        && layout.image_width > 0.0
        && layout.image_height > 0.0
    {
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
    if let (Some(image), Some(tool)) = (
        app.get_webview_window("result"),
        app.get_webview_window("result-tools"),
    ) && let (Ok(image_ptr), Ok(tool_ptr)) = (image.ns_window(), tool.ns_window())
    {
        screentrans_platform::detach_child_window(image_ptr, tool_ptr);
    }
}

#[cfg(target_os = "macos")]
fn attach_macos_child(app: &AppHandle, image: &WebviewWindow, tool: &WebviewWindow) {
    let _ = app;
    if let (Ok(image_ptr), Ok(tool_ptr)) = (image.ns_window(), tool.ns_window()) {
        screentrans_platform::attach_child_window(image_ptr, tool_ptr);
    }
}

struct PlacingGuard<'a>(&'a AppHandle);
impl Drop for PlacingGuard<'_> {
    fn drop(&mut self) {
        lock(&self.0.state::<AppState>().result_toolbar).placing = false;
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
    builder
        .inner_size(248.0, 48.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()
        .map_err(|_| windows::window_error())
}
fn place(app: &AppHandle, tool: &WebviewWindow, bounds: Rect) -> Result<()> {
    let target_x = bounds.x.round() as i32;
    let target_y = bounds.y.round() as i32;
    let target_w = bounds.width.round() as u32;
    let target_h = bounds.height.round() as u32;
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.placing = true;
        bar.expected = Some((target_x, target_y));
    }
    let _guard = PlacingGuard(app);
    let pos = tool.inner_position().map_err(|_| windows::window_error())?;
    let size = tool.inner_size().map_err(|_| windows::window_error())?;
    let pos_diff = (pos.x - target_x).abs() > 1 || (pos.y - target_y).abs() > 1;
    let size_diff = size.width.abs_diff(target_w) > 1 || size.height.abs_diff(target_h) > 1;
    if pos_diff || size_diff {
        #[cfg(target_os = "macos")]
        {
            let scale = tool.scale_factor().unwrap_or(1.0).max(1.0);
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
        #[cfg(not(target_os = "macos"))]
        {
            windows::set_client_rect(tool, bounds)?;
        }
        if let Ok(actual) = tool.inner_position() {
            lock(&app.state::<AppState>().result_toolbar).expected = Some((actual.x, actual.y));
        }
    }
    Ok(())
}
pub fn clamp_toolbar(app: &AppHandle) -> Result<()> {
    let Some(tool) = app.get_webview_window("result-tools") else {
        return Ok(());
    };
    let p = tool.inner_position().map_err(|_| windows::window_error())?;
    let s = tool.inner_size().map_err(|_| windows::window_error())?;
    let bounds = Rect {
        x: p.x as f64,
        y: p.y as f64,
        width: s.width as f64,
        height: s.height as f64,
    };
    let (area, _) = monitor(&tool, bounds)?;
    place(app, &tool, crate::result_toolbar::clamp(bounds, area))
}
pub fn sync(app: &AppHandle) -> Result<()> {
    if screentrans_platform::native_wayland_windows() {
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
        let rect = crate::result_toolbar::placement(bounds, scale, &settings, area);
        place(app, &tool, rect)?;
        lock(&state.result_toolbar).above = rect.bottom() <= bounds.y;
        #[cfg(target_os = "macos")]
        attach_macos_child(app, &image, &tool);
    }
    let _ = tool.emit(
        "toolbar-placement-changed",
        lock(&state.result_toolbar).clone(),
    );
    if !tool.is_visible().unwrap_or(false) {
        windows::request_show(&tool)?;
    }
    Ok(())
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
    }
    let state = app.state::<AppState>();
    let mut result = lock(&state.result_layout).clone();
    let bar = lock(&state.result_toolbar).clone();
    result.controls_above = bar.above;
    result.toolbar_align_left = bar.align_left;
    if screentrans_platform::native_wayland_windows() {
        *lock(&state.result_layout) = result.clone();
        let _ = app.emit_to("result", "result-layout-changed", result.clone());
    } else {
        sync(app)?;
    }
    Ok(result)
}
pub fn begin_drag(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.detached = true;
        bar.toolbar_moving = true;
        bar.expected = None;
        bar.generation += 1;
    }
    #[cfg(target_os = "macos")]
    detach_macos_child(app);
}
pub async fn finish_drag(app: &AppHandle) -> Result<()> {
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
    if screentrans_platform::native_wayland_windows() {
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
    lock(&state.result_toolbar).finish_image_gesture();
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
    if screentrans_platform::native_wayland_windows() {
        if *lock(&window.state::<AppState>().result_frame_id) == frame_id {
            finish_wayland_drag_gesture(window.app_handle(), None);
        }
        return Ok(());
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
    #[cfg(not(any(windows, target_os = "macos")))]
    tokio::time::sleep(Duration::from_millis(160)).await;
    if *lock(&window.state::<AppState>().result_frame_id) == frame_id {
        finish_image_gesture(window.app_handle())?;
    }
    Ok(())
}
pub fn quiet(app: &AppHandle, quiet: bool) -> Result<()> {
    lock(&app.state::<AppState>().result_toolbar).quiet = quiet;
    sync(app)
}
pub fn hide(app: &AppHandle) -> Result<()> {
    // Renderer cleanup is not guaranteed after hide/crash; release native resize
    // surfaces here as well as on normal pointer release and owner destruction.
    *lock(&app.state::<AppState>().result_resize) = None;
    {
        let state = app.state::<AppState>();
        let mut bar = lock(&state.result_toolbar);
        bar.finish_image_gesture();
        bar.placing = false;
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
    if screentrans_platform::native_wayland_windows() {
        #[cfg(target_os = "linux")]
        if label == "result" {
            schedule_wayland_drag_finish(window);
        }
        return;
    }
    {
        let state = window.state::<AppState>();
        let bar = lock(&state.result_toolbar);
        if label == "result" && (bar.image_moving || bar.image_resizing) {
            return;
        }
        if label == "result-tools"
            && (bar.toolbar_moving || bar.placing || bar.image_moving || bar.image_resizing)
        {
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
            let mut bar = lock(&state.result_toolbar);
            if bar.toolbar_moving || bar.placing || bar.matches_expected(pos.x, pos.y) {
                return;
            }
            bar.detached = true;
        }
        let generation = {
            let state = app.state::<AppState>();
            let mut bar = lock(&state.result_toolbar);
            if bar.toolbar_moving || bar.placing {
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
            bar.generation == generation && !bar.toolbar_moving && !bar.placing
        };
        if should_sync {
            let _ = sync(&app);
        }
    });
}

pub fn window_resized(window: &tauri::Window) {
    let label = window.label();
    if label != "result" && label != "result-tools" {
        return;
    }
    #[cfg(target_os = "linux")]
    if screentrans_platform::native_wayland_windows() {
        if label == "result" && lock(&window.state::<AppState>().result_resize).is_some() {
            schedule_wayland_resize_finish(window);
            return;
        }
        return;
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
