//! Diagnose OCR and overlay rendering from a saved screenshot without driving the desktop.
//! Usage: review_capture INPUT OUTPUT_DIR [x,y,width,height] [--translate]
use screentrans_core::{
    TranslationItem, TranslationRequest,
    config::{ProviderId, provider_defaults},
    layout,
    ports::{OcrBackend, TranslatorBackend},
    render,
    translators::HttpTranslator,
};
use screentrans_platform::ocr::SystemOcr;
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err(
            "Usage: review_capture INPUT OUTPUT_DIR [x,y,width,height] [--translate]".into(),
        );
    }
    let mut image = image::open(&args[1])?.to_rgba8();
    if let Some(crop) = args.get(3).filter(|arg| !arg.starts_with("--")) {
        let values: Vec<u32> = crop.split(',').map(str::parse).collect::<Result<_, _>>()?;
        if values.len() != 4
            || values[0] + values[2] > image.width()
            || values[1] + values[3] > image.height()
        {
            return Err("Crop is outside the image".into());
        }
        image = image::imageops::crop_imm(&image, values[0], values[1], values[2], values[3])
            .to_image();
    }
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    image.save(output.join("original.png"))?;
    let upscaled = image::imageops::resize(
        &image,
        image.width() * 2,
        image.height() * 2,
        image::imageops::FilterType::CatmullRom,
    );
    let mut lines = SystemOcr
        .recognize(
            Arc::new(upscaled),
            vec!["zh-Hans-CN".into(), "en-US".into()],
            CancellationToken::new(),
        )
        .await?;
    for line in &mut lines {
        line.bounds.x /= 2.0;
        line.bounds.y /= 2.0;
        line.bounds.width /= 2.0;
        line.bounds.height /= 2.0;
    }
    let mut blocks = layout::group_lines(lines);
    let online = args.iter().any(|arg| arg == "--translate");
    if online {
        let translator = HttpTranslator::new(
            ProviderId::GoogleFree,
            provider_defaults(ProviderId::GoogleFree),
            String::new(),
        )?;
        let result = translator
            .translate(
                TranslationRequest {
                    source: "auto".into(),
                    target: "en".into(),
                    items: blocks
                        .iter()
                        .map(|b| TranslationItem {
                            id: b.id.clone(),
                            text: b.text.clone(),
                        })
                        .collect(),
                },
                CancellationToken::new(),
            )
            .await?;
        for block in &mut blocks {
            block.translation = result
                .iter()
                .find(|item| item.id == block.id)
                .unwrap()
                .text
                .clone();
        }
    } else {
        for block in &mut blocks {
            block.translation = block.text.clone();
        }
    }
    let clean = render::process_visual_rendering(&image, &mut blocks);
    clean.save(output.join("clean.png"))?;
    let state = serde_json::json!({
        "source": if online { "google_free" } else { "identity-render-check" },
        "frame": { "id": "review-original", "cleanId": "review-clean", "width": image.width(), "height": image.height(), "screenBounds": null, "scaleFactor": 1 },
        "blocks": blocks,
    });
    std::fs::write(
        output.join("state.json"),
        serde_json::to_vec_pretty(&state)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&state)?);
    Ok(())
}
