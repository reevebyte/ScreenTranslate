use crate::state::lock;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, menu::MenuItem};
#[derive(Default)]
pub struct UpdateUi {
    pub busy: std::sync::atomic::AtomicBool,
    pub request: Mutex<Option<String>>,
    pub tray: Mutex<Option<MenuItem<tauri::Wry>>>,
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
    if !matches!(
        stage,
        "idle"
            | "current"
            | "checking"
            | "available"
            | "downloading"
            | "ready"
            | "installing"
            | "error"
    ) {
        return Err("无效的托盘更新状态".into());
    }
    let newer = version.is_some()
        && matches!(
            stage,
            "available" | "downloading" | "ready" | "installing" | "error"
        );
    let enabled = newer && !blocked && matches!(stage, "available" | "ready" | "error");
    let text = if newer {
        format!(
            "↑ v{} · {}",
            version.unwrap(),
            match stage {
                "downloading" =>
                    if english {
                        "Downloading…"
                    } else {
                        "下载中…"
                    },
                "installing" =>
                    if english {
                        "Installing…"
                    } else {
                        "安装中…"
                    },
                _ =>
                    if english {
                        "Update"
                    } else {
                        "更新"
                    },
            }
        )
    } else {
        format!("{} {current}", if english { "Version" } else { "版本" })
    };
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
        if let Some(item) = lock(&app.state::<UpdateUi>().tray).as_ref() {
            item.set_text(text).map_err(|e| e.to_string())?;
            item.set_enabled(enabled).map_err(|e| e.to_string())?;
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
        assert!(text.contains("↑ v2.0.3"));
        assert!(enabled);
        for stage in ["downloading", "installing"] {
            assert!(
                !presentation("2.0.2", stage, Some("2.0.3"), false, true)
                    .unwrap()
                    .1
            );
        }
        assert!(
            !presentation("2.0.2", "ready", Some("2.0.3"), true, true)
                .unwrap()
                .1
        );
        assert!(presentation("2.0.2", "available", Some("bad\nversion"), false, false).is_err());
    }
}
