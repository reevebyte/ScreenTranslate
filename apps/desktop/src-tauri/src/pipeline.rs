use crate::{
    state::{AppState, Sessions, lock},
    windows,
};
use screentrans_core::{
    AppError, ErrorCode, FrameMetadata, JobStage, JobStatus, Result, TranslationItem,
    TranslationRequest,
    config::{Config, OcrEngine},
    jobs::{JobRegistry, JobTicket},
    language, layout,
    ports::{OcrBackend, TranslatorBackend},
    translators::HttpTranslator,
};
use screentrans_platform::{
    cloud_ocr::{AzureOcr, YoudaoOcr},
    custom_ocr::CustomOcr,
    ocr::SystemOcr,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

fn translator(state: &AppState, config: &Config) -> Result<HttpTranslator> {
    let key = state
        .vault
        .get(&config.provider.key())
        .ok()
        .flatten()
        .unwrap_or_default();
    let options = config
        .providers
        .get(&config.provider)
        .cloned()
        .unwrap_or_default();
    HttpTranslator::new(config.provider, options, key)
}

pub fn schedule_text(app: AppHandle, delay: bool) {
    let state = app.state::<AppState>();
    let ticket = state.jobs.begin("text");
    let (revision, input) = {
        let mut sessions = lock(&state.sessions);
        let revision = sessions.text.revision;
        sessions.text.job = JobStatus {
            id: ticket.id.clone(),
            session_id: "text".into(),
            revision,
            stage: JobStage::Idle,
            error: None,
        };
        (revision, sessions.text.input.clone())
    };
    if input.trim().is_empty() {
        state.publish_text(&app);
        return;
    }
    tauri::async_runtime::spawn(async move {
        if delay {
            tokio::select! { _ = ticket.stop.cancelled() => return, _ = tokio::time::sleep(Duration::from_millis(500)) => {} }
        }
        let state = app.state::<AppState>();
        if !state.jobs.is_current("text", &ticket) {
            return;
        }
        {
            let mut sessions = lock(&state.sessions);
            if sessions.text.revision != revision || ticket.stop.is_cancelled() {
                return;
            }
            sessions.text.job.stage = JobStage::Translating;
        }
        state.publish_text(&app);
        let config = lock(&state.config).clone();
        let session = lock(&state.sessions).text.clone();
        let result = async {
            let backend = translator(&state, &config)?;
            let target = language::target_for(&input, &session.target, &config.chinese_target);
            let response = backend
                .translate(
                    TranslationRequest {
                        source: session.source,
                        target,
                        items: vec![TranslationItem {
                            id: "text".into(),
                            text: input,
                        }],
                    },
                    ticket.stop.clone(),
                )
                .await?;
            Ok::<_, AppError>(
                response
                    .into_iter()
                    .next()
                    .map(|item| item.text)
                    .unwrap_or_default(),
            )
        }
        .await;
        if !state.jobs.is_current("text", &ticket) {
            return;
        }
        {
            let mut sessions = lock(&state.sessions);
            if sessions.text.revision != revision || ticket.stop.is_cancelled() {
                return;
            }
            match result {
                Ok(output) => {
                    sessions.text.output = output;
                    sessions.text.job.stage = JobStage::Complete;
                }
                Err(error) => {
                    sessions.text.job.stage = JobStage::Error;
                    sessions.text.job.error = Some(error);
                }
            }
        }
        state.publish_text(&app);
    });
}

pub fn start_visual(app: AppHandle, metadata: FrameMetadata, recognize: bool) {
    let state = app.state::<AppState>();
    state.jobs.cancel_prefix("block:");
    let ticket = state.jobs.begin("visual");
    let revision = {
        let mut sessions = lock(&state.sessions);
        sessions.visual.revision += 1;
        let revision = sessions.visual.revision;
        sessions.visual.frame = Some(metadata.clone());
        if recognize {
            sessions.visual.blocks.clear();
            sessions.visual.input.clear();
            sessions.visual.output.clear();
        }
        sessions.visual.job = JobStatus {
            id: ticket.id.clone(),
            session_id: "visual".into(),
            revision,
            stage: if recognize {
                JobStage::Recognizing
            } else {
                JobStage::Translating
            },
            error: None,
        };
        revision
    };
    state.publish_visual(&app);
    if let Err(e) = windows::show_result(&app, &metadata) {
        tracing::error!("show_result failed: {e:?}");
        let _ = windows::show_main(&app);
    }
    tauri::async_runtime::spawn(async move {
        let outcome = tokio::time::timeout(
            Duration::from_secs(120),
            run_visual(&app, &metadata, recognize, revision, &ticket),
        )
        .await
        .unwrap_or_else(|_| {
            Err(AppError::new(
                ErrorCode::Network,
                "截图处理超时，请检查翻译服务后重试",
            ))
        });
        let state = app.state::<AppState>();
        if !state.jobs.is_current("visual", &ticket) {
            return;
        }
        {
            let mut sessions = lock(&state.sessions);
            if sessions.visual.revision != revision || ticket.stop.is_cancelled() {
                return;
            }
            match outcome {
                Ok(()) => sessions.visual.job.stage = JobStage::Complete,
                Err(error) => {
                    sessions.visual.job.stage = JobStage::Error;
                    sessions.visual.job.error = Some(error);
                }
            }
        }
        state.publish_visual(&app);
        let config = lock(&state.config).clone();
        if config.appearance.auto_copy
            && lock(&state.sessions).visual.job.stage == JobStage::Complete
        {
            let output = lock(&state.sessions).visual.output.clone();
            let _ = app.clipboard().write_text(output);
        }
    });
}

async fn run_visual(
    app: &AppHandle,
    metadata: &FrameMetadata,
    recognize: bool,
    revision: u32,
    ticket: &JobTicket,
) -> Result<()> {
    let state = app.state::<AppState>();
    let config = lock(&state.config).clone();
    if recognize {
        let image = lock(&state.frames).image(&metadata.id)?;
        let (image, ratio) = if config.ocr.upscale && image.width().max(image.height()) < 1200 {
            let resized = image::imageops::resize(
                image.as_ref(),
                image.width() * 2,
                image.height() * 2,
                image::imageops::FilterType::CatmullRom,
            );
            (Arc::new(resized), 2.0)
        } else {
            (image, 1.0)
        };
        let backend: Box<dyn OcrBackend> = match config.ocr.engine {
            OcrEngine::Youdao => Box::new(YoudaoOcr),
            OcrEngine::System => Box::new(SystemOcr),
            OcrEngine::Custom => Box::new(CustomOcr {
                options: config.ocr.custom.clone(),
                key: state
                    .vault
                    .get("custom_ocr")
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            }),
            OcrEngine::Azure => Box::new(AzureOcr {
                endpoint: config.ocr.azure_endpoint.clone(),
                key: state
                    .vault
                    .get("azure_ocr")
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            }),
        };
        let ocr_stop = ticket.stop.child_token();
        let mut lines = match tokio::time::timeout(
            Duration::from_secs(if config.ocr.engine == OcrEngine::Custom {
                90
            } else {
                30
            }),
            backend.recognize(image, config.ocr.languages.clone(), ocr_stop.clone()),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                ocr_stop.cancel();
                return Err(AppError::new(
                    ErrorCode::OcrUnavailable,
                    "文字识别超时，请缩小选区后重试",
                )
                .action("ocr"));
            }
        };
        for line in &mut lines {
            line.bounds.x /= ratio;
            line.bounds.y /= ratio;
            line.bounds.width /= ratio;
            line.bounds.height /= ratio;
        }
        if !state.jobs.is_current("visual", ticket) {
            return Err(AppError::cancelled());
        }
        let blocks = layout::group_lines(lines);
        if blocks.is_empty() {
            return Err(AppError::new(
                ErrorCode::NoText,
                "没有识别到文字，请重新框选或检查 OCR 语言",
            )
            .action("ocr"));
        }
        {
            let mut sessions = lock(&state.sessions);
            if sessions.visual.revision != revision || ticket.stop.is_cancelled() {
                return Err(AppError::cancelled());
            }
            sessions.visual.input = blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            sessions.visual.blocks = blocks;
            sessions.visual.job.stage = JobStage::Translating;
        }
        state.publish_visual(app);
    }
    let backend = translator(&state, &config)?;
    let blocks = lock(&state.sessions).visual.blocks.clone();
    let mut groups: BTreeMap<String, Vec<TranslationItem>> = BTreeMap::new();
    for block in blocks {
        let target = language::target_for(
            &block.text,
            if block.target_language.is_empty() {
                &config.target
            } else {
                &block.target_language
            },
            &config.chinese_target,
        );
        groups.entry(target).or_default().push(TranslationItem {
            id: block.id,
            text: block.text,
        });
    }
    for (target, items) in groups {
        let translated = backend
            .translate(
                TranslationRequest {
                    source: config.source.clone(),
                    target: target.clone(),
                    items,
                },
                ticket.stop.clone(),
            )
            .await?;
        if !state.jobs.is_current("visual", ticket) {
            return Err(AppError::cancelled());
        }
        {
            let mut sessions = lock(&state.sessions);
            if sessions.visual.revision != revision || ticket.stop.is_cancelled() {
                return Err(AppError::cancelled());
            }
            for item in translated {
                if let Some(block) = sessions
                    .visual
                    .blocks
                    .iter_mut()
                    .find(|block| block.id == item.id)
                {
                    block.translation = item.text;
                }
            }
            sessions.visual.output = sessions
                .visual
                .blocks
                .iter()
                .map(|block| block.translation.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
        }
        state.publish_visual(app);
    }

    crate::visual_render::render(
        &state.frames,
        &state.sessions,
        revision,
        ticket.stop.clone(),
    )
    .await?;
    state.publish_visual(app);
    Ok(())
}

fn begin_block_edit(
    sessions: &mut Sessions,
    jobs: &JobRegistry,
    id: &str,
    text: &str,
    target: &str,
) -> Result<(u32, String, JobTicket, u32)> {
    // A correction can arrive after a new capture has replaced its blocks.
    // Validate before cancelling that capture's current OCR/translation job.
    let block = sessions
        .visual
        .blocks
        .iter_mut()
        .find(|block| block.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "文本块已更新，请重新选择"))?;
    let visual_revision = sessions.visual.revision;
    let lane = format!("block:{visual_revision}:{id}");
    let ticket = jobs.begin(&lane);
    jobs.cancel("visual");
    block.revision += 1;
    block.text = text.into();
    block.target_language = target.into();
    block.translation.clear();
    let revision = block.revision;
    sessions.visual.job.stage = JobStage::Translating;
    sessions.visual.job.error = None;
    Ok((visual_revision, lane, ticket, revision))
}

pub fn retranslate_block(app: AppHandle, id: String, text: String, target: String) -> Result<()> {
    let state = app.state::<AppState>();
    let (visual_revision, lane, ticket, revision) =
        begin_block_edit(&mut lock(&state.sessions), &state.jobs, &id, &text, &target)?;
    state.publish_visual(&app);
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let config = lock(&state.config).clone();
        let result = async {
            let backend = translator(&state, &config)?;
            let response = backend
                .translate(
                    TranslationRequest {
                        source: config.source.clone(),
                        target: language::target_for(&text, &target, &config.chinese_target),
                        items: vec![TranslationItem {
                            id: id.clone(),
                            text,
                        }],
                    },
                    ticket.stop.clone(),
                )
                .await?;
            Ok::<_, AppError>(response[0].text.clone())
        }
        .await;
        if !state.jobs.is_current(&lane, &ticket) {
            return;
        }
        let error = {
            let mut sessions = lock(&state.sessions);
            if sessions.visual.revision != visual_revision || ticket.stop.is_cancelled() {
                return;
            }
            let Some(block) = sessions
                .visual
                .blocks
                .iter_mut()
                .find(|block| block.id == id && block.revision == revision)
            else {
                return;
            };
            match result {
                Ok(text) => {
                    block.translation = text;
                    None
                }
                Err(error) => Some(error),
            }
        };
        {
            let mut sessions = lock(&state.sessions);
            if sessions.visual.revision != visual_revision || ticket.stop.is_cancelled() {
                return;
            }
            sessions.visual.input = sessions
                .visual
                .blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            sessions.visual.output = sessions
                .visual
                .blocks
                .iter()
                .map(|b| b.translation.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            sessions.visual.job.stage = if error.is_some() {
                JobStage::Error
            } else {
                JobStage::Complete
            };
            sessions.visual.job.error = error;
        }

        let _ = crate::visual_render::render(
            &state.frames,
            &state.sessions,
            visual_revision,
            ticket.stop.clone(),
        )
        .await;
        state.publish_visual(&app);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use screentrans_core::{OcrLine, Rect};

    #[test]
    fn stale_block_correction_does_not_cancel_a_new_capture() {
        let jobs = JobRegistry::default();
        let active = jobs.begin("visual");
        let mut sessions = Sessions::default();
        sessions.visual.revision = 2;
        sessions.visual.job.stage = JobStage::Recognizing;

        assert!(begin_block_edit(&mut sessions, &jobs, "block-0", "old capture", "en").is_err());
        assert!(jobs.is_current("visual", &active));
        assert_eq!(sessions.visual.job.stage, JobStage::Recognizing);
        assert!(sessions.visual.blocks.is_empty());
    }

    #[test]
    fn valid_correction_belongs_to_the_locked_capture_revision() {
        let jobs = JobRegistry::default();
        let active = jobs.begin("visual");
        let text = jobs.begin("text");
        let mut sessions = Sessions::default();
        sessions.visual.revision = 7;
        sessions.visual.blocks = layout::group_lines(vec![OcrLine {
            text: "before".into(),
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
            },
            confidence: None,
        }]);
        let old_revision = sessions.visual.blocks[0].revision;
        let (visual_revision, lane, ticket, revision) =
            begin_block_edit(&mut sessions, &jobs, "block-0", "corrected", "en").unwrap();

        assert_eq!(visual_revision, 7);
        assert_eq!(lane, "block:7:block-0");
        assert_eq!(revision, old_revision + 1);
        assert_eq!(sessions.visual.blocks[0].text, "corrected");
        assert_eq!(sessions.visual.blocks[0].target_language, "en");
        assert_eq!(sessions.visual.job.stage, JobStage::Translating);
        assert!(!jobs.is_current("visual", &active));
        assert!(jobs.is_current("text", &text));
        assert!(jobs.is_current(&lane, &ticket));
    }
}
