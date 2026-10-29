use crate::{
    state::{AppState, lock},
    windows,
};
use screentrans_core::{Rect, Result};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

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
    lock(&app.state::<AppState>().result_toolbar).expected =
        Some((bounds.x.round() as i32, bounds.y.round() as i32));
    let pos = tool.inner_position().map_err(|_| windows::window_error())?;
    let size = tool.inner_size().map_err(|_| windows::window_error())?;
    if pos.x != bounds.x.round() as i32
        || pos.y != bounds.y.round() as i32
        || size.width != bounds.width.round() as u32
        || size.height != bounds.height.round() as u32
    {
        windows::set_client_rect(tool, bounds)?;
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
    let Some(image) = app.get_webview_window("result") else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let settings = lock(&state.result_toolbar).clone();
    if !image.is_visible().unwrap_or(false) || settings.suppressed() {
        if let Some(tool) = app.get_webview_window("result-tools") {
            let _ = tool.hide();
        }
        return Ok(());
    }
    let tool = prepare(app)?;
    if settings.detached {
        clamp_toolbar(app)?;
    } else {
        let bounds = image_bounds(&image)?;
        let (area, scale) = monitor(&image, bounds)?;
        let rect = crate::result_toolbar::placement(bounds, scale, &settings, area);
        place(app, &tool, rect)?;
        lock(&state.result_toolbar).above = rect.bottom() <= bounds.y;
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
    }
    sync(app)?;
    let state = app.state::<AppState>();
    let mut result = lock(&state.result_layout).clone();
    let bar = lock(&state.result_toolbar);
    result.controls_above = bar.above;
    result.toolbar_align_left = bar.align_left;
    Ok(result)
}
pub fn begin_drag(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut bar = lock(&state.result_toolbar);
    bar.detached = true;
    bar.expected = None;
}
pub fn begin_image_drag(app: &AppHandle) -> Result<()> {
    lock(&app.state::<AppState>().result_toolbar).image_moving = true;
    sync(app)
}
pub fn finish_image_gesture(app: &AppHandle) -> Result<()> {
    lock(&app.state::<AppState>().result_toolbar).finish_image_gesture();
    sync(app)
}
pub async fn finish_image_drag(window: &WebviewWindow) -> Result<()> {
    let frame_id = lock(&window.state::<AppState>().result_frame_id).clone();
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
    #[cfg(not(windows))]
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
    lock(&app.state::<AppState>().result_toolbar).finish_image_gesture();
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
    if label == "result" {
        let state = window.state::<AppState>();
        let bar = lock(&state.result_toolbar);
        if bar.image_moving || bar.image_resizing {
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
            let Ok(pos) = tool.inner_position() else {
                return;
            };
            let state = app.state::<AppState>();
            let mut bar = lock(&state.result_toolbar);
            if bar.expected == Some((pos.x, pos.y)) {
                return;
            }
            bar.detached = true;
        }
        let generation = {
            let state = app.state::<AppState>();
            let mut bar = lock(&state.result_toolbar);
            bar.generation += 1;
            bar.generation
        };
        tokio::time::sleep(Duration::from_millis(if label == "result" {
            20
        } else {
            120
        }))
        .await;
        if lock(&app.state::<AppState>().result_toolbar).generation == generation {
            let _ = sync(&app);
        }
    });
}
