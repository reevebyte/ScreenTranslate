use async_trait::async_trait;
use image::RgbaImage;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::{CaptureFrame, MonitorInfo, OcrLine, Result, TranslatedItem, TranslationRequest};

#[async_trait]
pub trait CaptureBackend: Send + Sync {
    fn monitors(&self) -> Result<Vec<MonitorInfo>>;
    async fn capture(
        &self,
        monitor: Option<String>,
        stop: CancellationToken,
    ) -> Result<CaptureFrame>;
}

#[async_trait]
pub trait OcrBackend: Send + Sync {
    async fn recognize(
        &self,
        image: Arc<RgbaImage>,
        languages: Vec<String>,
        stop: CancellationToken,
    ) -> Result<Vec<OcrLine>>;
}

#[async_trait]
pub trait TranslatorBackend: Send + Sync {
    async fn translate(
        &self,
        request: TranslationRequest,
        stop: CancellationToken,
    ) -> Result<Vec<TranslatedItem>>;
}

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
}
