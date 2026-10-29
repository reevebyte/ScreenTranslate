use screentrans_core::ports::OcrBackend;
use screentrans_platform::cloud_ocr::YoudaoOcr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
#[ignore = "sends only the repository's synthetic English fixture to Youdao"]
async fn recognizes_synthetic_fixture_online() {
    let image = image::load_from_memory(include_bytes!("fixtures/ocr-fixture.png"))
        .unwrap()
        .to_rgba8();
    let lines = YoudaoOcr
        .recognize(Arc::new(image), Vec::new(), CancellationToken::new())
        .await
        .unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line.text.contains("ScreenTranslate"))
    );
}
