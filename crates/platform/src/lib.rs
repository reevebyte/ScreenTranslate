pub mod capture;
pub mod cloud_ocr;
pub mod credentials;
pub mod custom_ocr;
#[cfg(target_os = "macos")]
mod macos;
pub mod ocr;
#[cfg(target_os = "linux")]
pub mod portal;

use screentrans_core::PlatformCapabilities;

pub fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("x11")
}

pub fn capabilities() -> PlatformCapabilities {
    let wayland = is_wayland();
    let mut notes = Vec::new();
    if wayland {
        notes.push(
            "Wayland 使用系统截图选择器，译文在独立结果窗显示；快捷键由桌面 Portal 管理".into(),
        );
    }
    let (local_ocr, ocr_languages) = ocr::available_languages();
    if !local_ocr {
        notes.push(if cfg!(target_os = "linux") {
            "未找到 Tesseract 或语言包。Ubuntu/Debian 可安装 tesseract-ocr、tesseract-ocr-eng、tesseract-ocr-chi-sim".into()
        } else { "未找到可用的系统 OCR 语言，请安装系统 OCR 语言包".into() });
    }
    if cfg!(target_os = "macos") {
        notes.push("首次截图需要 macOS 屏幕录制权限；拒绝后可在系统设置中重新授权".into());
    }
    let tray = !cfg!(target_os = "linux")
        || !std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase()
            .contains("gnome");
    PlatformCapabilities {
        platform: std::env::consts::OS.into(),
        wayland,
        screenshot: true,
        overlay: !wayland,
        global_shortcuts: !wayland,
        local_ocr,
        ocr_languages,
        tray,
        notes,
    }
}
