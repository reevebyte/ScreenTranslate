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

fn portal_shortcuts_unsupported() -> AppError {
    AppError::new(
        ErrorCode::Unsupported,
        "当前桌面未提供全局快捷键 Portal 服务；可在桌面配置自定义快捷键执行 --capture",
    )
    .action("permissions")
}

fn portal_shortcuts_unbound() -> AppError {
    AppError::new(
        ErrorCode::Configuration,
        "桌面快捷键服务已响应，但尚未分配快捷键；请在桌面设置中绑定或配置自定义快捷键执行 --capture",
    )
    .action("hotkeys")
}

fn classify_portal_error_message(message: &str) -> AppError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("serviceunknown")
        || lower.contains("unknowninterface")
        || lower.contains("unknownmethod")
        || lower.contains("unknownobject")
        || lower.contains("no such interface")
        || lower.contains("not supported")
    {
        return portal_shortcuts_unsupported();
    }
    if lower.contains("notallowed") || lower.contains("not allowed") || lower.contains("denied") {
        return AppError::new(
            ErrorCode::PermissionDenied,
            "桌面未授权全局快捷键，可在桌面设置中允许或配置桌面自定义快捷键",
        )
        .action("permissions");
    }
    AppError::new(
        ErrorCode::Internal,
        "无法连接桌面快捷键服务，可稍后重试或配置桌面自定义快捷键",
    )
    .action("permissions")
}

fn classify_portal_error(error: &ashpd::Error) -> AppError {
    match error {
        ashpd::Error::Response(ResponseError::Cancelled)
        | ashpd::Error::Portal(ashpd::PortalError::Cancelled(_)) => AppError::cancelled(),
        ashpd::Error::Portal(ashpd::PortalError::NotAllowed(_)) => AppError::new(
            ErrorCode::PermissionDenied,
            "桌面未授权全局快捷键，可在桌面设置中允许或配置桌面自定义快捷键",
        )
        .action("permissions"),
        other => classify_portal_error_message(&other.to_string()),
    }
}

fn shortcut_error(value: &str) -> AppError {
    AppError::new(ErrorCode::Configuration, format!("快捷键格式无效：{value}")).action("hotkeys")
}

fn preferred_trigger(value: &str) -> Option<String> {
    let mut trigger = Vec::new();
    for raw in value.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            return None;
        }
        let lower = part.to_ascii_lowercase();
        let upper = part.to_ascii_uppercase();
        let function_key = upper.len() > 1
            && upper.starts_with('F')
            && upper[1..]
                .chars()
                .all(|character| character.is_ascii_digit());
        let token = match lower.as_str() {
            "ctrl" | "control" => "CTRL".to_string(),
            "alt" => "ALT".to_string(),
            "shift" => "SHIFT".to_string(),
            "super" | "meta" | "win" | "logo" => "LOGO".to_string(),
            "space" => "space".to_string(),
            "enter" | "return" => "Return".to_string(),
            "tab" => "Tab".to_string(),
            "escape" | "esc" => "Escape".to_string(),
            "backspace" => "BackSpace".to_string(),
            "delete" => "Delete".to_string(),
            "arrowup" => "Up".to_string(),
            "arrowdown" => "Down".to_string(),
            "arrowleft" => "Left".to_string(),
            "arrowright" => "Right".to_string(),
            "pageup" => "Page_Up".to_string(),
            "pagedown" => "Page_Down".to_string(),
            "home" => "Home".to_string(),
            "end" => "End".to_string(),
            "insert" => "Insert".to_string(),
            _ if part.len() == 1
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric()) =>
            {
                part.to_ascii_lowercase()
            }
            _ if function_key => upper,
            _ => return None,
        };
        trigger.push(token);
    }
    (!trigger.is_empty()).then(|| trigger.join("+"))
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

/// Silent whole-desktop capture, the way other screenshot tools work on
/// Wayland: the compositor asks for permission at most once (GNOME offers to
/// remember it) and the selection then happens in our own overlay.
pub async fn desktop_screenshot(stop: CancellationToken) -> Result<image::RgbaImage> {
    let started_at = SystemTime::now();
    let request = Screenshot::request().interactive(false).modal(false).send();
    let outcome = tokio::select! {
        _ = stop.cancelled() => return Err(AppError::cancelled()),
        response = request => response.and_then(|r| r.response()),
    };
    let uri = match outcome {
        Ok(res) => res.uri().as_str().to_owned(),
        Err(
            ashpd::Error::Response(ResponseError::Cancelled)
            | ashpd::Error::Portal(ashpd::PortalError::Cancelled(_)),
        ) => return Err(AppError::cancelled()),
        Err(_) => return Err(desktop_capture_error()),
    };
    tokio::task::spawn_blocking(move || {
        let path = url::Url::parse(&uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .or_else(|| uri.starts_with('/').then(|| PathBuf::from(&uri)))
            .ok_or_else(desktop_capture_error)?;
        for _ in 0..10 {
            if let Some(decoded) = decode_image_file(&path) {
                // The portal wrote this file for us; keep the user's Pictures
                // folder free of capture leftovers.
                let created_now = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .is_ok_and(|modified| modified + Duration::from_secs(2) >= started_at);
                if created_now {
                    let _ = std::fs::remove_file(&path);
                }
                return Ok(decoded);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(desktop_capture_error())
    })
    .await
    .map_err(|_| desktop_capture_error())?
}

fn desktop_capture_error() -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "系统未允许截图；请在系统弹出的截图授权中选择允许，或在系统设置的隐私权限中允许本应用截图",
    )
    .action("permissions")
}

fn parse_slurp_geometry(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let (origin, size) = trimmed.split_once(' ')?;
    let (x, y) = origin.split_once(',')?;
    let (w, h) = size.split_once('x')?;
    let x: i32 = x.parse().ok()?;
    let y: i32 = y.parse().ok()?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some(format!("{x},{y} {w}x{h}"))
}

/// Fast-path region screenshot for wlroots / Hyprland / Sway using `slurp` +
/// `grim` (stdout PNG, zero temp files, no Portal permission dialogs).
/// Returns `None` when `slurp` or `grim` is not installed so callers can fall
/// back seamlessly to the XDG Screenshot portal.
pub async fn wlroots_screenshot(stop: CancellationToken) -> Option<Result<CaptureFrame>> {
    if !super::is_tiling_wayland_compositor() {
        return None;
    }
    let slurp_child = match tokio::process::Command::new("slurp")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return None,
    };
    let slurp_output = tokio::select! {
        _ = stop.cancelled() => return Some(Err(AppError::cancelled())),
        res = slurp_child.wait_with_output() => match res {
            Ok(output) => output,
            Err(_) => return None,
        },
    };
    if !slurp_output.status.success() {
        // User pressed Escape or right-clicked in slurp.
        return Some(Err(AppError::cancelled()));
    }
    let geometry = match parse_slurp_geometry(&String::from_utf8_lossy(&slurp_output.stdout)) {
        Some(geom) => geom,
        None => return Some(Err(AppError::cancelled())),
    };
    let grim_child = match tokio::process::Command::new("grim")
        .args(["-t", "png", "-g", &geometry, "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return None,
    };
    let grim_output = tokio::select! {
        _ = stop.cancelled() => return Some(Err(AppError::cancelled())),
        res = grim_child.wait_with_output() => match res {
            Ok(output) => output,
            Err(_) => return Some(Err(portal_error())),
        },
    };
    if !grim_output.status.success() || grim_output.stdout.is_empty() {
        return None;
    }
    let png_bytes = grim_output.stdout;
    let decoded = tokio::task::spawn_blocking(move || {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16000);
        limits.max_image_height = Some(16000);
        limits.max_alloc = Some(192 * 1024 * 1024);
        let mut reader = image::ImageReader::new(std::io::Cursor::new(png_bytes))
            .with_guessed_format()
            .ok()?;
        reader.limits(limits);
        Some(reader.decode().ok()?.to_rgba8())
    })
    .await
    .ok()
    .flatten()?;
    Some(Ok(CaptureFrame {
        image: decoded,
        screen_bounds: None,
        scale_factor: 1.0,
    }))
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
                let created_now = std::fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .is_ok_and(|modified| modified + Duration::from_secs(2) >= started_at);
                if created_now {
                    let _ = std::fs::remove_file(path);
                }
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
    let portal = GlobalShortcuts::new()
        .await
        .map_err(|err| classify_portal_error(&err))?;
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|err| classify_portal_error(&err))?;
    // Session exposes its object path through Serialize; path() is private in ashpd.
    let session_path = serde_json::to_value(&session)
        .map_err(|_| portal_shortcuts_unsupported())?
        .as_str()
        .ok_or_else(portal_shortcuts_unsupported)?
        .to_owned();
    let mut events = portal
        .receive_activated()
        .await
        .map_err(|err| classify_portal_error(&err))?;
    let entries = [
        ("capture", "截图翻译", hotkeys.capture),
        ("toggle", "显示或隐藏译文", hotkeys.toggle),
        ("quick", "快速文字翻译", hotkeys.quick),
        ("search", "快速查找", hotkeys.search),
    ];
    let shortcuts: Vec<_> = entries
        .iter()
        .filter(|(_, _, key)| !key.is_empty())
        .map(|(id, description, key)| {
            preferred_trigger(key)
                .map(|trigger| {
                    NewShortcut::new(*id, *description).preferred_trigger(Some(trigger.as_str()))
                })
                .ok_or_else(|| shortcut_error(key))
        })
        .collect::<Result<_>>()?;
    let binding = tokio::time::timeout(
        Duration::from_secs(120),
        portal.bind_shortcuts(&session, &shortcuts, None, Default::default()),
    );
    let bound = tokio::select! {
        _ = stop.cancelled() => { let _ = session.close().await; return Ok(()); }
        result = binding => result
            .map_err(|_| AppError::new(ErrorCode::Internal, "等待桌面快捷键授权超时，请重试").action("hotkeys"))?
            .map_err(|err| classify_portal_error(&err))?
            .response()
            .map_err(|err| classify_portal_error(&err))?,
    };
    if bound.shortcuts().is_empty() {
        let _ = session.close().await;
        return Err(portal_shortcuts_unbound());
    }
    let closed = session
        .receive_closed()
        .await
        .map_err(|err| classify_portal_error(&err))?;
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

#[cfg(test)]
mod tests {
    use super::{classify_portal_error_message, parse_slurp_geometry, preferred_trigger};
    use screentrans_core::ErrorCode;

    #[test]
    fn parses_valid_slurp_geometry_and_rejects_empty_or_malformed() {
        assert_eq!(
            parse_slurp_geometry("120,240 640x360\n").as_deref(),
            Some("120,240 640x360")
        );
        assert_eq!(
            parse_slurp_geometry("-1920,0 800x400").as_deref(),
            Some("-1920,0 800x400")
        );
        assert_eq!(parse_slurp_geometry("10,20 0x100"), None);
        assert_eq!(parse_slurp_geometry("selection cancelled"), None);
    }

    #[test]
    fn portal_triggers_use_the_freedesktop_shortcut_language() {
        assert_eq!(
            preferred_trigger("Ctrl+Alt+Q").as_deref(),
            Some("CTRL+ALT+q")
        );
        assert_eq!(
            preferred_trigger("Ctrl+Alt+Space").as_deref(),
            Some("CTRL+ALT+space")
        );
        assert_eq!(
            preferred_trigger("Win+Shift+F12").as_deref(),
            Some("LOGO+SHIFT+F12")
        );
        assert_eq!(preferred_trigger("Ctrl+Unknown"), None);
    }

    #[test]
    fn classifies_portal_error_messages_accurately() {
        assert_eq!(
            classify_portal_error_message("org.freedesktop.DBus.Error.UnknownInterface").code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            classify_portal_error_message("org.freedesktop.DBus.Error.ServiceUnknown").code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            classify_portal_error_message("Portal error: NotAllowed").code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            classify_portal_error_message("transient io error").code,
            ErrorCode::Internal
        );
    }
}
