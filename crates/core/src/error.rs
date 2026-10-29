use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Cancelled,
    InvalidInput,
    Configuration,
    Credentials,
    PermissionDenied,
    OcrUnavailable,
    NoText,
    Network,
    Authentication,
    RateLimited,
    InvalidResponse,
    Unsupported,
    Storage,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error, TS)]
#[error("{message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub action: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            action: None,
        }
    }

    pub fn action(mut self, action: &str) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "操作已取消")
    }

    pub fn storage() -> Self {
        Self::new(ErrorCode::Storage, "无法读取或保存本地配置，请检查目录权限").action("settings")
    }
}
