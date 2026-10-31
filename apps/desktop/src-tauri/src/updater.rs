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
fn parse_proxy_server(raw: &str) -> Option<tauri::Url> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let value = if raw.contains('=') {
        raw.split(';')
            .map(str::trim)
            .find_map(|s| s.strip_prefix("https="))
            .or_else(|| {
                raw.split(';')
                    .map(str::trim)
                    .find_map(|s| s.strip_prefix("http="))
            })?
            .trim()
    } else {
        raw.split(';').next()?.trim()
    };
    if value.is_empty() {
        return None;
    }
    let normalized = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let url = tauri::Url::parse(&normalized).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url)
}
#[cfg(windows)]
fn registry_proxy() -> Option<tauri::Url> {
    use windows::{
        Win32::System::Registry::{
            HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
        },
        core::w,
    };
    let mut enabled = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings"),
            w!("ProxyEnable"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut enabled as *mut _ as _),
            Some(&mut size),
        )
    };
    if status.is_err() || enabled == 0 {
        return None;
    }
    let mut buf = [0u16; 512];
    let mut bytes = (buf.len() * std::mem::size_of::<u16>()) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings"),
            w!("ProxyServer"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as _),
            Some(&mut bytes),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    parse_proxy_server(&String::from_utf16_lossy(&buf[..len]))
}
fn system_proxy() -> Option<tauri::Url> {
    #[cfg(windows)]
    if let Some(proxy) = registry_proxy() {
        return Some(proxy);
    }
    [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .into_iter()
    .find_map(|key| {
        std::env::var(key)
            .ok()
            .and_then(|val| parse_proxy_server(&val))
    })
}
fn proxy_candidates(
    primary: Option<tauri::Url>,
    fallback: Option<tauri::Url>,
) -> Vec<Option<tauri::Url>> {
    let mut candidates = Vec::with_capacity(2);
    candidates.push(primary);
    if !candidates.contains(&fallback) {
        candidates.push(fallback);
    }
    if !candidates.contains(&None) {
        candidates.push(None);
    }
    candidates
}
fn is_signature_error(error: &tauri_plugin_updater::Error) -> bool {
    matches!(
        error,
        tauri_plugin_updater::Error::Minisign(_)
            | tauri_plugin_updater::Error::Base64(_)
            | tauri_plugin_updater::Error::SignatureUtf8(_)
            | tauri_plugin_updater::Error::SignedVersionMismatch { .. }
            | tauri_plugin_updater::Error::MissingSignedVersion
    )
}
fn download_error(error: &tauri_plugin_updater::Error) -> AppError {
    if is_signature_error(error) {
        AppError::new(ErrorCode::InvalidResponse, "更新包签名校验失败，请重试")
    } else {
        AppError::new(ErrorCode::Network, "更新下载失败，请检查网络或代理后重试")
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
    let detected = system_proxy();
    let candidates = proxy_candidates(detected.clone(), None);
    let mut last_error = None;
    let mut checked = None;
    for candidate in candidates {
        let builder = app.updater_builder().timeout(Duration::from_secs(20));
        let builder = match candidate {
            Some(proxy) => builder.proxy(proxy),
            None => builder.no_proxy(),
        };
        let updater = builder
            .build()
            .map_err(|_| AppError::new(ErrorCode::Configuration, "更新器未配置"))?;
        match updater.check().await {
            Ok(update) => {
                checked = Some(update);
                last_error = None;
                break;
            }
            Err(tauri_plugin_updater::Error::ReleaseNotFound) => {
                return Err(AppError::new(ErrorCode::Network, "尚未发布可用更新"));
            }
            Err(error) => {
                last_error = Some(error);
            }
        }
    }
    if last_error.is_some() && checked.is_none() {
        return Err(AppError::new(
            ErrorCode::Network,
            "无法检查更新，请稍后重试",
        ));
    }
    let mut update = checked.flatten();
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
    let candidates = proxy_candidates(update.proxy.clone(), system_proxy());
    let mut last_error = None;
    for candidate in candidates {
        let mut attempt = update.clone();
        attempt.timeout = Some(Duration::from_secs(600));
        attempt.no_proxy = candidate.is_none();
        attempt.proxy = candidate;
        let mut downloaded = 0_u64;
        match attempt
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
        {
            Ok(bytes) => {
                pending.bytes = Some(bytes);
                return Ok(());
            }
            Err(error) => {
                let stop = is_signature_error(&error);
                last_error = Some(error);
                if stop {
                    break;
                }
            }
        }
    }
    Err(last_error.as_ref().map(download_error).unwrap_or_else(|| {
        AppError::new(ErrorCode::Network, "更新下载失败，请检查网络或代理后重试")
    }))
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
    #[test]
    fn parses_windows_and_url_proxy_formats_and_builds_fallback_candidates() {
        assert_eq!(
            parse_proxy_server("127.0.0.1:17890").unwrap().as_str(),
            "http://127.0.0.1:17890/"
        );
        assert_eq!(
            parse_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:17890")
                .unwrap()
                .port(),
            Some(17890)
        );
        assert!(parse_proxy_server("socks5://127.0.0.1:1080").is_none());
        assert!(parse_proxy_server("").is_none());
        let proxy = parse_proxy_server("127.0.0.1:17890");
        assert_eq!(proxy_candidates(proxy.clone(), None), vec![proxy, None]);
        assert_eq!(proxy_candidates(None, None), vec![None]);
    }
    #[test]
    fn distinguishes_network_failures_from_signature_failures() {
        let net_err = download_error(&tauri_plugin_updater::Error::Network("reset".into()));
        assert_eq!(net_err.code, ErrorCode::Network);
        assert_eq!(net_err.message, "更新下载失败，请检查网络或代理后重试");
        let sig_err = download_error(&tauri_plugin_updater::Error::MissingSignedVersion);
        assert_eq!(sig_err.code, ErrorCode::InvalidResponse);
        assert_eq!(sig_err.message, "更新包签名校验失败，请重试");
    }
}
