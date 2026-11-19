use async_trait::async_trait;
use screentrans_core::{
    AppError, CaptureFrame, ErrorCode, MonitorInfo, Rect, Result, ports::CaptureBackend,
};
use tokio_util::sync::CancellationToken;

pub struct SystemCapture;

#[async_trait]
impl CaptureBackend for SystemCapture {
    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        if super::native_wayland_windows() {
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
        // Native Wayland windows cannot host our overlay, so the compositor's
        // own picker selects the region. Otherwise xcap captures the monitor
        // (on a Wayland session it tries GNOME Shell, the portal and wlroots).
        #[cfg(target_os = "linux")]
        if super::native_wayland_windows() {
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

impl SystemCapture {
    /// Captures every listed monitor, in the same order, for the selection
    /// overlays. On a Wayland session one silent desktop screenshot is split
    /// into monitors instead of asking the compositor once per screen.
    pub async fn capture_all(
        &self,
        monitors: &[MonitorInfo],
        stop: CancellationToken,
    ) -> Result<Vec<CaptureFrame>> {
        #[cfg(target_os = "linux")]
        if super::is_wayland() && !super::native_wayland_windows() {
            match super::portal::desktop_screenshot(stop.clone()).await {
                Ok(image) => {
                    if let Some(frames) = crop_monitors(&image, monitors) {
                        return Ok(frames);
                    }
                }
                Err(error) if error.code == ErrorCode::Cancelled => return Err(error),
                Err(_) => {}
            }
            // xcap serialises its Wayland captures on D-Bus, so go one by one.
            let mut frames = Vec::with_capacity(monitors.len());
            for info in monitors {
                frames.push(self.capture(Some(info.id.clone()), stop.clone()).await?);
            }
            return Ok(frames);
        }
        // Start every screen together, then collect in monitor order.
        let tasks: Vec<_> = monitors
            .iter()
            .map(|info| {
                let id = info.id.clone();
                let stop = stop.clone();
                tokio::spawn(async move { SystemCapture.capture(Some(id), stop).await })
            })
            .collect();
        let mut frames = Vec::with_capacity(tasks.len());
        for task in tasks {
            frames.push(
                task.await.map_err(|_| {
                    AppError::new(ErrorCode::Internal, "截屏任务异常结束，请重试")
                })??,
            );
        }
        Ok(frames)
    }
}

/// Lets the compositor's own screenshot UI pick the region (Wayland portal).
/// The frame has no screen position, so its result opens centred.
pub async fn picker_screenshot(stop: CancellationToken) -> Result<CaptureFrame> {
    #[cfg(target_os = "linux")]
    return super::portal::screenshot(stop).await;
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stop;
        Err(AppError::new(
            ErrorCode::Unsupported,
            "当前系统不支持系统截图选择器",
        ))
    }
}

/// Splits one screenshot of the whole desktop into per-monitor frames. The
/// image covers the bounding box of all monitors; each crop is resized to its
/// monitor's bounds so the overlay windows keep the geometry used to place
/// them. Returns `None` when the image cannot match the layout.
pub fn crop_monitors(
    image: &image::RgbaImage,
    monitors: &[MonitorInfo],
) -> Option<Vec<CaptureFrame>> {
    if monitors.is_empty() || image.width() == 0 || image.height() == 0 {
        return None;
    }
    let left = monitors
        .iter()
        .map(|m| m.bounds.x)
        .fold(f64::INFINITY, f64::min);
    let top = monitors
        .iter()
        .map(|m| m.bounds.y)
        .fold(f64::INFINITY, f64::min);
    let right = monitors
        .iter()
        .map(|m| m.bounds.x + m.bounds.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = monitors
        .iter()
        .map(|m| m.bounds.y + m.bounds.height)
        .fold(f64::NEG_INFINITY, f64::max);
    let (span_x, span_y) = (right - left, bottom - top);
    if !(span_x >= 1.0 && span_y >= 1.0) {
        return None;
    }
    let (full_w, full_h) = (f64::from(image.width()), f64::from(image.height()));
    let scale_x = full_w / span_x;
    let scale_y = full_h / span_y;
    // A different aspect ratio means the screenshot does not show this layout.
    if (scale_x / scale_y - 1.0).abs() > 0.02 {
        return None;
    }
    monitors
        .iter()
        .map(|monitor| {
            let bounds = monitor.bounds;
            let x0 = ((bounds.x - left) * scale_x).round().clamp(0.0, full_w);
            let y0 = ((bounds.y - top) * scale_y).round().clamp(0.0, full_h);
            let x1 = ((bounds.x + bounds.width - left) * scale_x)
                .round()
                .clamp(0.0, full_w);
            let y1 = ((bounds.y + bounds.height - top) * scale_y)
                .round()
                .clamp(0.0, full_h);
            let (width, height) = ((x1 - x0) as u32, (y1 - y0) as u32);
            if width == 0 || height == 0 {
                return None;
            }
            let crop =
                image::imageops::crop_imm(image, x0 as u32, y0 as u32, width, height).to_image();
            let target = (bounds.width.round() as u32, bounds.height.round() as u32);
            let image = if target.0 > 0 && target.1 > 0 && target != (width, height) {
                image::imageops::resize(
                    &crop,
                    target.0,
                    target.1,
                    image::imageops::FilterType::CatmullRom,
                )
            } else {
                crop
            };
            Some(CaptureFrame {
                image,
                screen_bounds: Some(bounds),
                scale_factor: monitor.scale_factor,
            })
        })
        .collect()
}

fn capture_error() -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "无法获取屏幕图像，请检查屏幕录制权限或显示器状态",
    )
    .action("permissions")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: &str, x: f64, y: f64, width: f64, height: f64) -> MonitorInfo {
        MonitorInfo {
            id: id.into(),
            name: id.into(),
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
            scale_factor: 1.0,
            primary: id == "a",
        }
    }

    fn desktop(width: u32, height: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 251) as u8, (y % 251) as u8, 0, 255])
        })
    }

    #[test]
    fn a_single_monitor_keeps_the_whole_screenshot() {
        let frames = crop_monitors(
            &desktop(1920, 1080),
            &[monitor("a", 0.0, 0.0, 1920.0, 1080.0)],
        )
        .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].image.dimensions(), (1920, 1080));
        assert_eq!(frames[0].screen_bounds.unwrap().width, 1920.0);
    }

    #[test]
    fn side_by_side_monitors_are_split_in_order() {
        let image = desktop(3840, 1080);
        let frames = crop_monitors(
            &image,
            &[
                monitor("b", 1920.0, 0.0, 1920.0, 1080.0),
                monitor("a", 0.0, 0.0, 1920.0, 1080.0),
            ],
        )
        .unwrap();
        assert_eq!(frames[0].screen_bounds.unwrap().x, 1920.0);
        assert_eq!(frames[0].image.get_pixel(0, 0), image.get_pixel(1920, 0));
        assert_eq!(frames[1].image.get_pixel(5, 7), image.get_pixel(5, 7));
    }

    #[test]
    fn negative_origins_and_mixed_heights_map_from_the_bounding_box() {
        let image = desktop(2560 + 1920, 1440);
        let frames = crop_monitors(
            &image,
            &[
                monitor("a", 0.0, 0.0, 1920.0, 1080.0),
                monitor("left", -2560.0, -360.0, 2560.0, 1440.0),
            ],
        )
        .unwrap();
        assert_eq!(frames[0].image.dimensions(), (1920, 1080));
        assert_eq!(frames[0].image.get_pixel(0, 0), image.get_pixel(2560, 360));
        assert_eq!(frames[1].image.dimensions(), (2560, 1440));
    }

    #[test]
    fn a_hidpi_screenshot_is_resized_to_the_monitor_bounds() {
        let frames = crop_monitors(
            &desktop(3840, 2160),
            &[monitor("a", 0.0, 0.0, 1920.0, 1080.0)],
        )
        .unwrap();
        assert_eq!(frames[0].image.dimensions(), (1920, 1080));
    }

    #[test]
    fn a_screenshot_that_does_not_match_the_layout_is_rejected() {
        assert!(
            crop_monitors(
                &desktop(1000, 1000),
                &[monitor("a", 0.0, 0.0, 1920.0, 1080.0)]
            )
            .is_none()
        );
        assert!(crop_monitors(&desktop(1920, 1080), &[]).is_none());
    }
}
