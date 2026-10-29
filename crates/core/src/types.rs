use image::RgbaImage;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppError, ErrorCode, Result};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub fn validate(self) -> Result<Self> {
        if [self.x, self.y, self.width, self.height]
            .iter()
            .any(|v| !v.is_finite())
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "选区尺寸无效，请重新框选",
            ));
        }
        Ok(self)
    }

    pub fn crop_bounds(self, width: u32, height: u32) -> Result<(u32, u32, u32, u32)> {
        self.validate()?;
        let left = self.x.floor().clamp(0.0, width as f64) as u32;
        let top = self.y.floor().clamp(0.0, height as f64) as u32;
        let right = self.right().ceil().clamp(0.0, width as f64) as u32;
        let bottom = self.bottom().ceil().clamp(0.0, height as f64) as u32;
        if right <= left || bottom <= top {
            return Err(AppError::new(ErrorCode::InvalidInput, "选区在图像范围之外"));
        }
        Ok((left, top, right - left, bottom - top))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    /// Native desktop coordinates: physical pixels on Windows/X11, points on macOS.
    pub bounds: Rect,
    pub scale_factor: f64,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FrameMetadata {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Native desktop rectangle. Image/OCR coordinates are always physical image pixels.
    /// None for imported images and Wayland screenshots with no global origin.
    pub screen_bounds: Option<Rect>,
    pub scale_factor: f64,
    #[serde(default)]
    pub clean_id: Option<String>,
}

pub struct CaptureFrame {
    pub image: RgbaImage,
    pub screen_bounds: Option<Rect>,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    pub id: String,
    pub bounds: Rect,
    #[serde(default)]
    pub source_lines: Vec<Rect>,
    pub text: String,
    pub translation: String,
    pub target_language: String,
    pub confidence: Option<f32>,
    pub revision: u32,
    #[serde(default)]
    pub background_color: Option<String>,
    #[serde(default)]
    pub foreground_color: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub line_height: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct OcrLine {
    pub text: String,
    pub bounds: Rect,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    Idle,
    Capturing,
    Recognizing,
    Translating,
    Complete,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub id: String,
    pub session_id: String,
    pub revision: u32,
    pub stage: JobStage,
    pub error: Option<AppError>,
}

impl Default for JobStatus {
    fn default() -> Self {
        Self {
            id: String::new(),
            session_id: "text".into(),
            revision: 0,
            stage: JobStage::Idle,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSession {
    pub id: String,
    pub revision: u32,
    pub client_id: String,
    pub client_revision: u32,
    pub input: String,
    pub output: String,
    pub source: String,
    pub target: String,
    pub frame: Option<FrameMetadata>,
    pub blocks: Vec<TextBlock>,
    pub job: JobStatus,
}

impl Default for TranslationSession {
    fn default() -> Self {
        Self {
            id: "text".into(),
            revision: 0,
            client_id: String::new(),
            client_revision: 0,
            input: String::new(),
            output: String::new(),
            source: "auto".into(),
            target: "auto".into(),
            frame: None,
            blocks: Vec::new(),
            job: JobStatus::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub platform: String,
    pub wayland: bool,
    pub screenshot: bool,
    pub overlay: bool,
    pub global_shortcuts: bool,
    pub local_ocr: bool,
    pub ocr_languages: Vec<String>,
    pub tray: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranslationItem {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranslatedItem {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub source: String,
    pub target: String,
    pub items: Vec<TranslationItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub version: String,
    pub initial_route: String,
    pub config: crate::config::Config,
    pub capabilities: PlatformCapabilities,
    pub text: TranslationSession,
    pub visual: TranslationSession,
    pub credentials: std::collections::BTreeMap<String, bool>,
    pub warnings: Vec<String>,
    pub monitors: Vec<MonitorInfo>,
    pub legacy_available: bool,
    pub updater_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub credentials_imported: u32,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_clamps_and_rounds_outward() {
        let rect = Rect {
            x: -2.2,
            y: 4.5,
            width: 14.0,
            height: 30.0,
        };
        assert_eq!(rect.crop_bounds(20, 20).unwrap(), (0, 4, 12, 16));
    }

    #[test]
    fn rejects_invalid_and_empty_selections() {
        for rect in [
            Rect {
                x: f64::NAN,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Rect {
                x: 50.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Rect::default(),
        ] {
            assert!(rect.crop_bounds(20, 20).is_err());
        }
    }
}
