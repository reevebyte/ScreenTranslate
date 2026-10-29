use async_trait::async_trait;
use screentrans_core::{
    AppError, CaptureFrame, ErrorCode, MonitorInfo, Rect, Result, ports::CaptureBackend,
};
use tokio_util::sync::CancellationToken;

pub struct SystemCapture;

#[async_trait]
impl CaptureBackend for SystemCapture {
    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        if super::is_wayland() {
            return Ok(Vec::new());
        }
        xcap::Monitor::all()
            .map_err(|_| capture_error())?
            .into_iter()
            .map(|monitor| {
                Ok(MonitorInfo {
                    id: monitor.id().map_err(|_| capture_error())?.to_string(),
                    name: monitor.name().unwrap_or_else(|_| "Display".into()),
                    bounds: Rect {
                        x: monitor.x().map_err(|_| capture_error())? as f64,
                        y: monitor.y().map_err(|_| capture_error())? as f64,
                        width: monitor.width().map_err(|_| capture_error())? as f64,
                        height: monitor.height().map_err(|_| capture_error())? as f64,
                    },
                    scale_factor: monitor.scale_factor().unwrap_or(1.0) as f64,
                    primary: monitor.is_primary().unwrap_or(false),
                })
            })
            .collect()
    }

    async fn capture(
        &self,
        monitor_id: Option<String>,
        stop: CancellationToken,
    ) -> Result<CaptureFrame> {
        if stop.is_cancelled() {
            return Err(AppError::cancelled());
        }
        #[cfg(target_os = "linux")]
        if super::is_wayland() {
            return super::portal::screenshot(stop).await;
        }
        let capture = tokio::task::spawn_blocking(move || {
            let monitors = xcap::Monitor::all().map_err(|_| capture_error())?;
            let monitor = monitors
                .into_iter()
                .find(|m| {
                    monitor_id.as_ref().map_or_else(
                        || m.is_primary().unwrap_or(false),
                        |id| m.id().is_ok_and(|value| value.to_string() == *id),
                    )
                })
                .ok_or_else(|| {
                    AppError::new(ErrorCode::InvalidInput, "显示器已断开，请重新截图")
                })?;
            let x = monitor.x().map_err(|_| capture_error())? as f64;
            let y = monitor.y().map_err(|_| capture_error())? as f64;
            let scale = monitor.scale_factor().unwrap_or(1.0) as f64;
            #[cfg(target_os = "macos")]
            let image = super::macos::capture(monitor.id().map_err(|_| capture_error())?)?;
            #[cfg(not(target_os = "macos"))]
            let image = monitor.capture_image().map_err(|_| capture_error())?;
            let bounds = Rect {
                x,
                y,
                width: monitor.width().map_err(|_| capture_error())? as f64,
                height: monitor.height().map_err(|_| capture_error())? as f64,
            };
            Ok(CaptureFrame {
                image,
                screen_bounds: Some(bounds),
                scale_factor: scale,
            })
        });
        tokio::select! {
            _ = stop.cancelled() => Err(AppError::cancelled()),
            result = capture => result.map_err(|_| capture_error())?,
        }
    }
}

fn capture_error() -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "无法获取屏幕图像，请检查屏幕录制权限或显示器状态",
    )
    .action("permissions")
}
