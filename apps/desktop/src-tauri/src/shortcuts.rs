#[cfg(target_os = "linux")]
use crate::state::{AppState, lock};
use crate::windows;
use screentrans_core::{AppError, ErrorCode, Result, config::Hotkeys};
use tauri::AppHandle;
#[cfg(target_os = "linux")]
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn dispatch(app: &AppHandle, action: &str) {
    match action {
        "capture" => windows::begin_capture(app.clone(), None),
        "quick" => {
            let _ = windows::toggle_quick(app);
        }
        "toggle" => {
            let _ = windows::toggle_result(app);
        }
        _ => {}
    }
}

pub fn apply(app: &AppHandle, keys: &Hotkeys) -> Result<()> {
    if cfg!(feature = "e2e") || screentrans_platform::is_wayland() {
        return Ok(());
    }
    let entries = [
        ("capture", &keys.capture),
        ("toggle", &keys.toggle),
        ("quick", &keys.quick),
    ];
    let parsed: Vec<_> = entries
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(action, value)| {
            value
                .parse::<Shortcut>()
                .map(|key| (action, key))
                .map_err(|_| {
                    AppError::new(ErrorCode::Configuration, format!("快捷键格式无效：{value}"))
                        .action("hotkeys")
                })
        })
        .collect::<Result<_>>()?;
    app.global_shortcut()
        .unregister_all()
        .map_err(|_| shortcut_error())?;
    for (action, shortcut) in parsed {
        app.global_shortcut()
            .on_shortcut(shortcut, move |app, _, event| {
                if event.state == ShortcutState::Pressed {
                    dispatch(app, action);
                }
            })
            .map_err(|_| shortcut_error())?;
    }
    Ok(())
}

fn shortcut_error() -> AppError {
    AppError::new(
        ErrorCode::Configuration,
        "快捷键已被其他程序占用，请重新设置",
    )
    .action("hotkeys")
}

pub fn configure_portal(app: AppHandle, keys: Hotkeys) {
    #[cfg(target_os = "linux")]
    {
        let state = app.state::<AppState>();
        lock(&state.portal_stop).cancel();
        let stop = tokio_util::sync::CancellationToken::new();
        *lock(&state.portal_stop) = stop.clone();
        tauri::async_runtime::spawn(async move {
            let callback_app = app.clone();
            let callback = std::sync::Arc::new(move |action: String| {
                if action == "ready" {
                    lock(&callback_app.state::<AppState>().capabilities).global_shortcuts = true;
                    let _ = callback_app.emit("capabilities-changed", ());
                } else {
                    dispatch(&callback_app, &action);
                }
            });
            if let Err(error) = screentrans_platform::portal::shortcuts(keys, callback, stop).await
            {
                app.state::<AppState>().warning(&app, error.message);
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, keys);
    }
}
