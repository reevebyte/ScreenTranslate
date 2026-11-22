//! Adapter bridging the standalone `lumen-ocr` crate (`reevebyte/LumenOCR`)
//! into ScreenTranslate's `OcrBackend`, `AppError`, and `LocalOcrStatus` types.

use async_trait::async_trait;
use image::RgbaImage;
use lumen_ocr::{ErrorKind, InstallState, LumenError, LumenManager, Status};
use screentrans_core::{
    AppError, ErrorCode, LocalOcrInstallState, LocalOcrStatus, OcrLine, Rect, Result,
    ports::OcrBackend,
};
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub use lumen_ocr::{parse_worker_args, worker_main};

fn map_error(error: LumenError) -> AppError {
    let code = match error.kind {
        ErrorKind::Cancelled => ErrorCode::Cancelled,
        ErrorKind::InvalidInput => ErrorCode::InvalidInput,
        ErrorKind::Configuration => ErrorCode::Configuration,
        ErrorKind::OcrUnavailable => ErrorCode::OcrUnavailable,
        ErrorKind::Network => ErrorCode::Network,
        ErrorKind::InvalidResponse => ErrorCode::InvalidResponse,
        ErrorKind::Unsupported => ErrorCode::Unsupported,
        ErrorKind::Storage => ErrorCode::Storage,
        ErrorKind::Internal => ErrorCode::Internal,
    };
    let mapped = AppError::new(code, error.message);
    if code == ErrorCode::Cancelled {
        mapped
    } else {
        mapped.action("ocr")
    }
}

fn map_status(status: Status) -> LocalOcrStatus {
    LocalOcrStatus {
        supported: status.supported,
        state: match status.state {
            InstallState::NotInstalled => LocalOcrInstallState::NotInstalled,
            InstallState::Downloading => LocalOcrInstallState::Downloading,
            InstallState::Ready => LocalOcrInstallState::Ready,
            InstallState::Error => LocalOcrInstallState::Error,
        },
        model_version: status.model_version,
        installed_bytes: status.installed_bytes,
        downloaded_bytes: status.downloaded_bytes,
        total_bytes: status.total_bytes,
        worker_running: status.worker_running,
        error: status.error,
    }
}

fn map_line(line: lumen_ocr::OcrLine) -> OcrLine {
    OcrLine {
        text: line.text,
        bounds: Rect {
            x: line.bounds.x,
            y: line.bounds.y,
            width: line.bounds.width,
            height: line.bounds.height,
        },
        confidence: line.confidence,
    }
}

pub struct RapidOcrManager {
    inner: Arc<LumenManager>,
}

impl RapidOcrManager {
    pub fn new(model_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: LumenManager::new(model_dir),
        })
    }

    pub fn with_worker_exe(model_dir: PathBuf, worker_exe: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: LumenManager::with_worker_exe(model_dir, worker_exe),
        })
    }

    pub fn set_idle_unload_secs(&self, secs: u32) {
        self.inner.set_idle_unload_secs(secs);
    }

    pub fn is_downloading(&self) -> bool {
        self.inner.is_downloading()
    }

    pub fn status(&self) -> LocalOcrStatus {
        map_status(self.inner.status())
    }

    pub async fn download<F>(&self, mut on_status: F) -> Result<LocalOcrStatus>
    where
        F: FnMut(LocalOcrStatus) + Send,
    {
        self.inner
            .download(move |status| on_status(map_status(status)))
            .await
            .map(map_status)
            .map_err(map_error)
    }

    pub fn cancel_download(&self) {
        self.inner.cancel_download();
    }

    pub async fn remove(&self) -> Result<LocalOcrStatus> {
        self.inner.remove().await.map(map_status).map_err(map_error)
    }

    pub async fn shutdown_worker(&self) {
        self.inner.shutdown_worker().await;
    }

    pub fn shutdown_blocking(&self) {
        self.inner.shutdown_blocking();
    }

    pub fn backend(self: &Arc<Self>, idle_unload_secs: u32) -> RapidOcrBackend {
        self.set_idle_unload_secs(idle_unload_secs);
        RapidOcrBackend {
            inner: Arc::clone(&self.inner),
        }
    }
}

pub struct RapidOcrBackend {
    inner: Arc<LumenManager>,
}

#[async_trait]
impl OcrBackend for RapidOcrBackend {
    async fn recognize(
        &self,
        image: Arc<RgbaImage>,
        _languages: Vec<String>,
        stop: CancellationToken,
    ) -> Result<Vec<OcrLine>> {
        self.inner
            .recognize(image, stop)
            .await
            .map(|lines| lines.into_iter().map(map_line).collect())
            .map_err(map_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_recoverable_ocr_error_when_models_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let manager = RapidOcrManager::new(dir.path().to_path_buf());
        let status = manager.status();
        assert_eq!(status.state, LocalOcrInstallState::NotInstalled);
        assert!(!status.worker_running);

        let backend = manager.backend(60);
        let err = backend
            .recognize(
                Arc::new(RgbaImage::new(32, 32)),
                vec![],
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::OcrUnavailable);
        assert_eq!(err.action.as_deref(), Some("ocr"));
    }
}
