use screentrans_core::ports::OcrBackend;
use screentrans_platform::rapid_ocr::RapidOcrManager;
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

/// End-to-end test of the isolated `--ocr-worker` child process against
/// `tests/fixtures/ocr-fixture.png`.
///
/// Requires:
/// - `SCREENTRANS_RAPID_OCR_DIR`: directory containing the downloaded PP-OCRv5 models
/// - `SCREENTRANS_WORKER_EXE`: path to the built `screentranslate` executable
#[tokio::test]
#[ignore = "requires downloaded PP-OCRv5 models and built desktop executable"]
async fn recognizes_fixture_image_through_worker_process() {
    let dir = PathBuf::from(
        std::env::var("SCREENTRANS_RAPID_OCR_DIR")
            .expect("set SCREENTRANS_RAPID_OCR_DIR to a directory with PP-OCRv5 models"),
    );
    let worker_bin = PathBuf::from(
        std::env::var("SCREENTRANS_WORKER_EXE")
            .expect("set SCREENTRANS_WORKER_EXE to the built screentranslate executable"),
    );
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr-fixture.png");
    let image = Arc::new(
        image::open(&fixture)
            .expect("load ocr-fixture.png")
            .to_rgba8(),
    );

    let manager = RapidOcrManager::with_worker_exe(dir, worker_bin);
    let backend = manager.backend(60);

    let lines = backend
        .recognize(
            image,
            vec!["zh-Hans-CN".into(), "en-US".into()],
            CancellationToken::new(),
        )
        .await
        .expect("recognize fixture via worker");

    assert!(!lines.is_empty(), "expected non-empty OCR lines");
    let joined = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.contains("ScreenTranslate") && joined.contains("Read the world."),
        "unexpected OCR output: {joined}"
    );
    assert!(manager.status().worker_running);

    manager.shutdown_worker().await;
    assert!(!manager.status().worker_running);
}
