use image::RgbaImage;
use screentrans_core::{AppError, ErrorCode, OcrLine, Rect, Result};
use std::sync::OnceLock;
use windows::{
    Globalization::Language,
    Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
};

pub fn languages() -> (bool, Vec<String>) {
    if !keep_runtime_alive() {
        return (false, Vec::new());
    }
    let _apartment = Apartment::new();
    let list: Vec<_> = OcrEngine::AvailableRecognizerLanguages()
        .ok()
        .into_iter()
        .flat_map(|list| list.into_iter())
        .filter_map(|language| language.LanguageTag().ok().map(|tag| tag.to_string()))
        .collect();
    (!list.is_empty(), list)
}

fn keep_runtime_alive() -> bool {
    // windows-rs caches agile activation factories process-wide. RoUninitialize on the
    // last temporary OCR thread can unload their DLL while the cached vtable survives.
    // Pin the MTA for the process lifetime, independently of individual worker apartments.
    static MTA_COOKIE: OnceLock<std::result::Result<usize, ()>> = OnceLock::new();
    MTA_COOKIE
        .get_or_init(|| unsafe {
            windows::Win32::System::Com::CoIncrementMTAUsage()
                .map(|cookie| cookie.0 as usize)
                .map_err(|_| ())
        })
        .is_ok()
}

struct Apartment(bool);
impl Apartment {
    fn new() -> Self {
        let initialized = unsafe {
            windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            )
            .is_ok()
        };
        Self(initialized)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::WinRT::RoUninitialize();
            }
        }
    }
}

pub fn recognize(image: &RgbaImage, languages: &[String]) -> Result<Vec<OcrLine>> {
    if !keep_runtime_alive() {
        return Err(AppError::new(
            ErrorCode::OcrUnavailable,
            "无法初始化 Windows OCR，请重启应用后重试",
        )
        .action("ocr"));
    }
    let _apartment = Apartment::new();
    let unavailable = || {
        AppError::new(
            ErrorCode::OcrUnavailable,
            "请在 Windows 语言选项中安装所需的 OCR 语言包",
        )
        .action("ocr")
    };
    let engine = languages
        .iter()
        .find_map(|tag| {
            let language = Language::CreateLanguage(&tag.as_str().into()).ok()?;
            OcrEngine::TryCreateFromLanguage(&language).ok()
        })
        .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
        .ok_or_else(unavailable)?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let ratio = (max as f64 / image.width().max(image.height()) as f64).min(1.0);
    let smaller = if ratio < 1.0 {
        image::imageops::resize(
            image,
            (image.width() as f64 * ratio) as u32,
            (image.height() as f64 * ratio) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image.clone()
    };
    let mut bgra = smaller.as_raw().clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let fail = || {
        AppError::new(
            ErrorCode::OcrUnavailable,
            "Windows OCR 识别失败，请检查语言包或缩小选区",
        )
        .action("ocr")
    };
    let writer = DataWriter::new().map_err(|_| fail())?;
    writer.WriteBytes(&bgra).map_err(|_| fail())?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &writer.DetachBuffer().map_err(|_| fail())?,
        BitmapPixelFormat::Bgra8,
        smaller.width() as i32,
        smaller.height() as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(|_| fail())?;
    let recognized = engine
        .RecognizeAsync(&bitmap)
        .map_err(|_| fail())?
        .get()
        .map_err(|_| fail())?;
    let mut lines = Vec::new();
    for line in recognized.Lines().map_err(|_| fail())? {
        let words = line.Words().map_err(|_| fail())?;
        let bounds = screentrans_core::layout::union(
            words
                .into_iter()
                .filter_map(|word| word.BoundingRect().ok())
                .map(|rect| Rect {
                    x: rect.X as f64 / ratio,
                    y: rect.Y as f64 / ratio,
                    width: rect.Width as f64 / ratio,
                    height: rect.Height as f64 / ratio,
                }),
        );
        if let Some(bounds) = bounds {
            lines.push(OcrLine {
                text: line.Text().map_err(|_| fail())?.to_string(),
                bounds,
                confidence: None,
            });
        }
    }
    Ok(lines)
}
