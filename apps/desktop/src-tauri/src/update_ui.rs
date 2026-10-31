use crate::state::lock;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, menu::MenuItem};
pub struct TrayItems {
    pub capture: MenuItem<tauri::Wry>,
    pub quick: MenuItem<tauri::Wry>,
    pub toggle: MenuItem<tauri::Wry>,
    pub main: MenuItem<tauri::Wry>,
    pub update: MenuItem<tauri::Wry>,
    pub quit: MenuItem<tauri::Wry>,
}
#[derive(Default)]
pub struct UpdateUi {
    pub busy: std::sync::atomic::AtomicBool,
    pub request: Mutex<Option<String>>,
    pub tray: Mutex<Option<TrayItems>>,
}
pub async fn on_ui<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(work(handle));
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|_| "窗口操作未完成".to_string())?
}
pub fn request(app: &AppHandle, source: &str) {
    *lock(&app.state::<UpdateUi>().request) = Some(source.to_string());
    if source == "tray" {
        let _ = crate::windows::show_main(app);
    }
    let _ = app.emit_to("main", "app-update-requested", ());
}
#[cfg(windows)]
pub fn caption_colors(app: &AppHandle) -> (u32, u32) {
    let theme = app
        .try_state::<crate::state::AppState>()
        .map(|state| {
            let saved = lock(&state.config).appearance.theme;
            lock(&state.theme_preview).effective(saved)
        })
        .unwrap_or(screentrans_core::config::Theme::System);
    let (_, background, text) = crate::windows::caption_palette(theme);
    (background, text)
}
#[tauri::command]
pub async fn take_update_request(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("请在主窗口管理软件更新".into());
    }
    Ok(lock(&app.state::<UpdateUi>().request).take())
}
fn presentation(
    current: &str,
    stage: &str,
    version: Option<&str>,
    blocked: bool,
    english: bool,
) -> Result<(String, bool), String> {
    if version.is_some_and(|s| s.is_empty() || s.len() > 64 || s.chars().any(char::is_control)) {
        return Err("无效的更新版本".into());
    }
    match stage {
        "idle" | "current" | "checking" | "error" => {}
        "available" | "downloading" | "ready" | "installing" if version.is_some() => {}
        _ => return Err("无效的托盘更新状态".into()),
    }
    let target = if matches!(stage, "idle" | "current" | "checking") {
        current
    } else {
        version.unwrap_or(current)
    };
    let enabled = version.is_some() && !blocked && matches!(stage, "available" | "ready" | "error");
    let text = format!("{} {target}", if english { "Version" } else { "版本" });
    Ok((text, enabled))
}
#[tauri::command]
pub async fn set_tray_update(
    app: AppHandle,
    window: WebviewWindow,
    stage: String,
    version: Option<String>,
    blocked: bool,
    english: bool,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请在主窗口管理软件更新".into());
    }
    app.state::<UpdateUi>().busy.store(
        matches!(stage.as_str(), "checking" | "downloading" | "installing"),
        std::sync::atomic::Ordering::Relaxed,
    );
    let (text, enabled) = presentation(
        env!("CARGO_PKG_VERSION"),
        &stage,
        version.as_deref(),
        blocked,
        english,
    )?;
    on_ui(&app, move |app| {
        if let Some(items) = lock(&app.state::<UpdateUi>().tray).as_ref() {
            items
                .capture
                .set_text(if english {
                    "Capture and translate"
                } else {
                    "截图翻译"
                })
                .map_err(|e| e.to_string())?;
            items
                .quick
                .set_text(if english {
                    "Quick translate"
                } else {
                    "快速文字翻译"
                })
                .map_err(|e| e.to_string())?;
            items
                .toggle
                .set_text(if english {
                    "Hide / show"
                } else {
                    "显示 / 隐藏译文"
                })
                .map_err(|e| e.to_string())?;
            items
                .main
                .set_text(if english {
                    "Open main window"
                } else {
                    "打开主窗口"
                })
                .map_err(|e| e.to_string())?;
            items.update.set_text(text).map_err(|e| e.to_string())?;
            items
                .update
                .set_enabled(enabled)
                .map_err(|e| e.to_string())?;
            items
                .quit
                .set_text(if english { "Quit" } else { "退出" })
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
}
#[cfg(not(windows))]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn set_update_caption(
    _window: WebviewWindow,
    visible: bool,
    enabled: bool,
    mode: Option<String>,
    progress: Option<u32>,
    version: Option<String>,
    english: Option<bool>,
) -> Result<(), String> {
    let _ = (visible, enabled, mode, progress, version, english);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_is_quiet_until_new_release_and_blocked_during_work() {
        assert_eq!(
            presentation("2.0.2", "idle", None, false, false).unwrap(),
            ("版本 2.0.2".into(), false)
        );
        let (text, enabled) =
            presentation("2.0.2", "available", Some("2.0.3"), false, false).unwrap();
        assert_eq!(text, "版本 2.0.3");
        assert!(enabled);
        assert_eq!(
            presentation("2.0.2", "available", Some("2.0.3"), false, true).unwrap(),
            ("Version 2.0.3".into(), true)
        );
        for stage in ["downloading", "installing"] {
            assert_eq!(
                presentation("2.0.2", stage, Some("2.0.3"), false, true).unwrap(),
                ("Version 2.0.3".into(), false)
            );
        }
        assert!(
            !presentation("2.0.2", "ready", Some("2.0.3"), true, true)
                .unwrap()
                .1
        );
        assert_eq!(
            presentation("2.0.2", "error", None, false, false).unwrap(),
            ("版本 2.0.2".into(), false)
        );
        assert!(presentation("2.0.2", "ready", None, false, false).is_err());
        assert!(presentation("2.0.2", "available", Some("bad\nversion"), false, false).is_err());
    }
}
