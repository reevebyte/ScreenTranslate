use screentrans_core::ports::OcrBackend;
use screentrans_platform::ocr::SystemOcr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Requires an installed OS OCR language / Tesseract. No screenshots, credentials, or network are used.
#[tokio::test]
#[ignore = "requires a local OCR engine and English language data"]
async fn recognizes_public_fixture_with_image_pixel_coordinates() {
    let image = image::load_from_memory(include_bytes!("fixtures/ocr-fixture.png"))
        .unwrap()
        .to_rgba8();
    let width = image.width() as f64;
    let height = image.height() as f64;
    let lines = SystemOcr
        .recognize(
            Arc::new(image),
            vec!["en-US".into()],
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(text.contains("screentranslate"), "{text}");
    assert!(text.contains("world"), "{text}");
    for line in lines {
        assert!(line.bounds.x >= 0.0 && line.bounds.y >= 0.0);
        assert!(line.bounds.right() <= width + 1.0 && line.bounds.bottom() <= height + 1.0);
    }
}

/// The application checks OCR languages before submitting recognition work to another thread.
/// One-shot OCR tests miss cached WinRT factories outliving the apartment that created them.
#[cfg(windows)]
#[tokio::test]
#[ignore = "requires Windows OCR and English language data"]
async fn language_detection_then_repeated_ocr_survives_thread_teardown() {
    let image = Arc::new(
        image::load_from_memory(include_bytes!("fixtures/ocr-fixture.png"))
            .unwrap()
            .to_rgba8(),
    );
    for _ in 0..3 {
        let (available, _) = std::thread::spawn(screentrans_platform::ocr::available_languages)
            .join()
            .unwrap();
        assert!(available);
        let lines = SystemOcr
            .recognize(
                image.clone(),
                vec!["en-US".into()],
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line.text.to_lowercase().contains("screentranslate"))
        );
    }
}
