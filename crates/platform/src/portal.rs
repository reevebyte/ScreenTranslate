use ashpd::desktop::{
    ResponseError,
    global_shortcuts::{GlobalShortcuts, NewShortcut},
    screenshot::Screenshot,
};
use futures_util::StreamExt;
use screentrans_core::{AppError, CaptureFrame, ErrorCode, Result, config::Hotkeys};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio_util::sync::CancellationToken;

fn portal_error() -> AppError {
    AppError::new(
        ErrorCode::Unsupported,
        "桌面 Portal 未提供此功能或授权已取消；可使用主窗口按钮、打开图片或桌面自定义快捷键",
    )
    .action("permissions")
}

fn decode_image_file(path: &Path) -> Option<image::RgbaImage> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 64 * 1024 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    Some(reader.decode().ok()?.to_rgba8())
}

fn recent_screenshot_dirs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    if let Ok(config) = std::fs::read_to_string(home.join(".config/user-dirs.dirs")) {
        for line in config.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("XDG_PICTURES_DIR=") {
                let value = rest.trim_matches('"');
                let pic_dir = if let Some(rel) = value.strip_prefix("$HOME/") {
                    home.join(rel)
                } else {
                    PathBuf::from(value)
                };
                dirs.push(pic_dir.join("Screenshots"));
                dirs.push(pic_dir.join("截图"));
                dirs.push(pic_dir);
            }
        }
    }
    for sub in ["Pictures/Screenshots", "图片/截图", "Pictures", "图片"] {
        let candidate = home.join(sub);
        if !dirs.contains(&candidate) {
            dirs.push(candidate);
        }
    }
    dirs
}

fn find_recent_screenshot(since: SystemTime) -> Option<image::RgbaImage> {
    let threshold = since
        .checked_sub(Duration::from_millis(500))
        .unwrap_or(since);
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for dir in recent_screenshot_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_image = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp"
                    )
                });
            if !is_image {
                continue;
            }
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
                && meta.len() > 0
                && let Ok(modified) = meta.modified()
                && modified >= threshold
                && newest.as_ref().is_none_or(|(best, _)| modified > *best)
            {
                newest = Some((modified, path));
            }
        }
    }
    let (_, path) = newest?;
    decode_image_file(&path)
}

pub async fn screenshot(stop: CancellationToken) -> Result<CaptureFrame> {
    let started_at = SystemTime::now();
    let request = Screenshot::request().interactive(true).send();
    let outcome = tokio::select! {
        _ = stop.cancelled() => return Err(AppError::cancelled()),
        response = request => response.and_then(|r| r.response()),
    };
    let uri = match outcome {
        Ok(res) => Some(res.uri().as_str().to_owned()),
        Err(
            ashpd::Error::Response(ResponseError::Cancelled)
            | ashpd::Error::Portal(ashpd::PortalError::Cancelled(_)),
        ) => return Err(AppError::cancelled()),
        Err(_) => None,
    };
    let image = tokio::task::spawn_blocking(move || {
        let portal_path = uri.as_deref().and_then(|raw| {
            url::Url::parse(raw)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .or_else(|| raw.starts_with('/').then(|| PathBuf::from(raw)))
        });
        for _ in 0..10 {
            if let Some(ref path) = portal_path
                && let Some(decoded) = decode_image_file(path)
            {
                return Ok(decoded);
            }
            if let Some(decoded) = find_recent_screenshot(started_at) {
                return Ok(decoded);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(portal_error())
    })
    .await
    .map_err(|_| portal_error())??;
    Ok(CaptureFrame {
        image,
        screen_bounds: None,
        scale_factor: 1.0,
    })
}

pub async fn shortcuts(
    hotkeys: Hotkeys,
    handler: Arc<dyn Fn(String) + Send + Sync>,
    stop: CancellationToken,
) -> Result<()> {
    let portal = GlobalShortcuts::new().await.map_err(|_| portal_error())?;
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|_| portal_error())?;
    // Session exposes its object path through Serialize; path() is private in ashpd.
    let session_path = serde_json::to_value(&session)
        .map_err(|_| portal_error())?
        .as_str()
        .ok_or_else(portal_error)?
        .to_owned();
    let mut events = portal
        .receive_activated()
        .await
        .map_err(|_| portal_error())?;
    let entries = [
        ("capture", "截图翻译", hotkeys.capture),
        ("toggle", "显示或隐藏译文", hotkeys.toggle),
        ("quick", "快速文字翻译", hotkeys.quick),
    ];
    let shortcuts: Vec<_> = entries
        .iter()
        .filter(|(_, _, key)| !key.is_empty())
        .map(|(id, description, key)| {
            NewShortcut::new(*id, *description).preferred_trigger(Some(key.as_str()))
        })
        .collect();
    let binding = tokio::time::timeout(
        Duration::from_secs(120),
        portal.bind_shortcuts(&session, &shortcuts, None, Default::default()),
    );
    tokio::select! {
        _ = stop.cancelled() => { let _ = session.close().await; return Ok(()); }
        result = binding => result
    .map_err(|_| portal_error())?
    .map_err(|_| portal_error())?
    .response()
    .map_err(|_| portal_error())?,
    };
    let closed = session.receive_closed().await.map_err(|_| portal_error())?;
    futures_util::pin_mut!(closed);
    handler("ready".into());
    loop {
        tokio::select! {
            _ = stop.cancelled() => { let _ = session.close().await; break; }
            _ = closed.next() => break,
            event = events.next() => match event {
                Some(event) if event.session_handle().as_str() == session_path => handler(event.shortcut_id().into()),
                Some(_) => {},
                None => break,
            }
        }
    }
    Ok(())
}
