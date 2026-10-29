use ashpd::desktop::{
    global_shortcuts::{GlobalShortcuts, NewShortcut},
    screenshot::Screenshot,
};
use futures_util::StreamExt;
use screentrans_core::{AppError, CaptureFrame, ErrorCode, Result, config::Hotkeys};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn portal_error() -> AppError {
    AppError::new(
        ErrorCode::Unsupported,
        "桌面 Portal 未提供此功能或授权已取消；可使用主窗口按钮、打开图片或桌面自定义快捷键",
    )
    .action("permissions")
}

pub async fn screenshot(stop: CancellationToken) -> Result<CaptureFrame> {
    let request = Screenshot::request().interactive(true).modal(true).send();
    let response = tokio::select! {
        _ = stop.cancelled() => return Err(AppError::cancelled()),
        response = request => response.map_err(|_| portal_error())?.response().map_err(|_| portal_error())?,
    };
    let uri = response.uri().as_str().to_owned();
    let image = tokio::task::spawn_blocking(move || {
        let url = url::Url::parse(&uri).map_err(|_| portal_error())?;
        let path = url.to_file_path().map_err(|_| portal_error())?;
        let metadata = std::fs::metadata(&path).map_err(|_| portal_error())?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            return Err(portal_error());
        }
        let reader = image::ImageReader::open(path).map_err(|_| portal_error())?;
        let mut reader = reader.with_guessed_format().map_err(|_| portal_error())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16000);
        limits.max_image_height = Some(16000);
        limits.max_alloc = Some(192 * 1024 * 1024);
        reader.limits(limits);
        reader
            .decode()
            .map(|image| image.to_rgba8())
            .map_err(|_| portal_error())
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
