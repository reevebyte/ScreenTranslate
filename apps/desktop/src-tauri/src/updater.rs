//! Signed updates: retain metadata and verified bytes together until explicit installation.
use crate::state::{AppState, lock};
use screentrans_core::{AppError, ErrorCode, JobStage, Result};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct PendingUpdate(Mutex<Pending>);
#[derive(Default)]
struct Pending {
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}

pub fn public_key() -> &'static str {
    option_env!("SCREENTRANS_UPDATE_PUBLIC_KEY")
        .filter(|key| !key.trim().is_empty())
        .unwrap_or(include_str!("../updater.pub"))
        .trim()
}
fn authorize(label: &str) -> Result<()> {
    if label != "main" {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在主窗口管理软件更新",
        ));
    }
    if public_key().is_empty() {
        return Err(AppError::new(ErrorCode::Unsupported, "未配置更新签名").action("releases"));
    }
    Ok(())
}
fn install_allowed(dirty: bool, stages: &[JobStage], capturing: bool) -> Result<()> {
    if dirty {
        return Err(AppError::new(
            ErrorCode::Configuration,
            "请先保存当前配置，再安装更新",
        ));
    }
    if capturing
        || stages.iter().any(|stage| {
            matches!(
                stage,
                JobStage::Capturing | JobStage::Recognizing | JobStage::Translating
            )
        })
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "请等待截图或翻译完成，再安装更新",
        ));
    }
    Ok(())
}
fn info(pending: &Pending) -> serde_json::Value {
    match &pending.update {
        Some(update) => {
            serde_json::json!({"available":true,"version":update.version,"notes":update.body,"downloaded":pending.bytes.is_some()})
        }
        None => serde_json::json!({"available":false}),
    }
}
#[tauri::command]
pub async fn check_update(app: AppHandle, window: WebviewWindow) -> Result<serde_json::Value> {
    authorize(window.label())?;
    let state = app.state::<PendingUpdate>();
    let mut pending = state.0.lock().await;
    if pending.bytes.is_some() {
        return Ok(info(&pending));
    }
    let mut update = app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| AppError::new(ErrorCode::Configuration, "更新器未配置"))?
        .check()
        .await
        .map_err(|error| match error {
            tauri_plugin_updater::Error::ReleaseNotFound => {
                AppError::new(ErrorCode::Network, "尚未发布可用更新")
            }
            _ => AppError::new(ErrorCode::Network, "无法检查更新，请稍后重试"),
        })?;
    if let Some(update) = update.as_mut() {
        update.timeout = Some(Duration::from_secs(600));
    }
    pending.update = update;
    Ok(info(&pending))
}
#[tauri::command]
pub async fn download_update(app: AppHandle, window: WebviewWindow, version: String) -> Result<()> {
    authorize(window.label())?;
    let state = app.state::<PendingUpdate>();
    let mut pending = state.0.lock().await;
    let update = pending
        .update
        .as_ref()
        .filter(|update| update.version == version)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "更新版本已变化，请重新检查"))?;
    if pending.bytes.is_some() {
        return Ok(());
    }
    let mut downloaded = 0_u64;
    let bytes = update
        .download(
            |length, total| {
                downloaded = downloaded.saturating_add(length as u64);
                let _ = app.emit_to(
                    "main",
                    "update-progress",
                    serde_json::json!({"downloaded":downloaded,"total":total}),
                );
            },
            || {},
        )
        .await
        .map_err(|_| AppError::new(ErrorCode::InvalidResponse, "更新下载或签名校验失败，请重试"))?;
    pending.bytes = Some(bytes);
    Ok(())
}
#[tauri::command]
pub async fn install_update(app: AppHandle, window: WebviewWindow, version: String) -> Result<()> {
    authorize(window.label())?;
    let pending_state = app.state::<PendingUpdate>();
    let pending = pending_state.0.lock().await;
    let state = app.state::<AppState>();
    let sessions = lock(&state.sessions);
    install_allowed(
        !lock(&state.dirty_windows).is_empty(),
        &[sessions.text.job.stage, sessions.visual.job.stage],
        !lock(&state.selections).is_empty(),
    )?;
    drop(sessions);
    let update = pending
        .update
        .as_ref()
        .filter(|update| update.version == version)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "更新版本已变化，请重新检查"))?;
    let bytes = pending
        .bytes
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "请先下载更新"))?;
    update
        .install(bytes)
        .map_err(|_| AppError::new(ErrorCode::Internal, "无法启动更新安装，请重试"))?;
    #[cfg(not(windows))]
    app.restart();
    #[cfg(windows)]
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_main_can_manage_updates_and_install_requires_idle_saved_work() {
        assert!(authorize("result").is_err());
        assert!(authorize("quick").is_err());
        assert!(install_allowed(true, &[JobStage::Idle], false).is_err());
        assert!(install_allowed(false, &[JobStage::Translating], false).is_err());
        assert!(install_allowed(false, &[JobStage::Recognizing], false).is_err());
        assert!(install_allowed(false, &[JobStage::Complete], true).is_err());
        assert!(install_allowed(false, &[JobStage::Complete], false).is_ok());
    }
}
