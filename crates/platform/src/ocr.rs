use async_trait::async_trait;
use image::RgbaImage;
#[cfg(not(target_os = "linux"))]
use screentrans_core::ErrorCode;
use screentrans_core::{AppError, OcrLine, Result, ports::OcrBackend};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "linux")]
#[path = "ocr_linux.rs"]
mod linux_ocr;
#[cfg(windows)]
#[path = "ocr_windows.rs"]
mod windows_ocr;

pub struct SystemOcr;

pub fn available_languages() -> (bool, Vec<String>) {
    #[cfg(windows)]
    {
        windows_ocr::languages()
    }
    #[cfg(target_os = "macos")]
    {
        (
            true,
            vec![
                "zh-Hans".into(),
                "en-US".into(),
                "ja-JP".into(),
                "ko-KR".into(),
            ],
        )
    }
    #[cfg(target_os = "linux")]
    {
        linux_ocr::languages()
    }
}

#[async_trait]
impl OcrBackend for SystemOcr {
    async fn recognize(
        &self,
        image: Arc<RgbaImage>,
        languages: Vec<String>,
        stop: CancellationToken,
    ) -> Result<Vec<OcrLine>> {
        if stop.is_cancelled() {
            return Err(AppError::cancelled());
        }
        #[cfg(target_os = "linux")]
        {
            linux_ocr::recognize(image, languages, stop).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let work = tokio::task::spawn_blocking(move || {
                #[cfg(windows)]
                {
                    windows_ocr::recognize(&image, &languages)
                }
                #[cfg(target_os = "macos")]
                {
                    super::macos::recognize(&image, &languages)
                }
            });
            tokio::select! {
                _ = stop.cancelled() => Err(AppError::cancelled()),
                result = work => result.map_err(|_| AppError::new(ErrorCode::Internal, "OCR 工作线程异常"))?,
            }
        }
    }
}
