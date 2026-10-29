use image::RgbaImage;
use screentrans_core::{AppError, ErrorCode, OcrLine, Rect, Result};
use std::{
    ffi::{CStr, CString},
    io::Cursor,
};

unsafe extern "C" {
    fn st_capture_display(
        display: u32,
        length: *mut usize,
        error: *mut *mut std::ffi::c_char,
    ) -> *mut u8;
    fn st_recognize(
        png: *const u8,
        length: usize,
        languages: *const std::ffi::c_char,
    ) -> *mut std::ffi::c_char;
    fn st_free(pointer: *mut std::ffi::c_void);
}

pub fn capture(display: u32) -> Result<RgbaImage> {
    let mut length = 0;
    let mut error = std::ptr::null_mut();
    unsafe {
        let pointer = st_capture_display(display, &mut length, &mut error);
        if pointer.is_null() {
            if !error.is_null() {
                st_free(error.cast());
            }
            return Err(AppError::new(
                ErrorCode::PermissionDenied,
                "macOS 未能截图，请允许屏幕录制权限后重试",
            )
            .action("permissions"));
        }
        let result = image::load_from_memory(std::slice::from_raw_parts(pointer, length));
        st_free(pointer.cast());
        result
            .map(|image| image.to_rgba8())
            .map_err(|_| AppError::new(ErrorCode::Internal, "无法解码系统截图"))
    }
}

pub fn recognize(image: &RgbaImage, languages: &[String]) -> Result<Vec<OcrLine>> {
    let fail = || {
        AppError::new(
            ErrorCode::OcrUnavailable,
            "Apple Vision 识别失败，请检查图像或 OCR 语言",
        )
        .action("ocr")
    };
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| fail())?;
    let languages =
        CString::new(serde_json::to_string(languages).map_err(|_| fail())?).map_err(|_| fail())?;
    unsafe {
        let result = st_recognize(
            png.get_ref().as_ptr(),
            png.get_ref().len(),
            languages.as_ptr(),
        );
        if result.is_null() {
            return Err(fail());
        }
        let json = CStr::from_ptr(result).to_bytes().to_vec();
        st_free(result.cast());
        let value: serde_json::Value = serde_json::from_slice(&json).map_err(|_| fail())?;
        let array = value.as_array().ok_or_else(fail)?;
        array
            .iter()
            .map(|line| {
                Ok(OcrLine {
                    text: line["text"].as_str().ok_or_else(fail)?.into(),
                    bounds: Rect {
                        x: line["x"].as_f64().ok_or_else(fail)?,
                        y: line["y"].as_f64().ok_or_else(fail)?,
                        width: line["width"].as_f64().ok_or_else(fail)?,
                        height: line["height"].as_f64().ok_or_else(fail)?,
                    },
                    confidence: line["confidence"].as_f64().map(|v| v as f32),
                })
            })
            .collect()
    }
}
