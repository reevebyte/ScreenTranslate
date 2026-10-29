use crate::{
    cache::{FrameCache, RenderedImage},
    state::{Sessions, lock},
};
use screentrans_core::{AppError, ErrorCode, Result, TextBlock};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub async fn render(
    frames: &Mutex<FrameCache>,
    sessions: &Mutex<Sessions>,
    revision: u32,
    stop: CancellationToken,
) -> Result<()> {
    let (frame_id, snapshot) = {
        let sessions = lock(sessions);
        if sessions.visual.revision != revision || stop.is_cancelled() {
            return Err(AppError::cancelled());
        }
        let Some(frame) = &sessions.visual.frame else {
            return Ok(());
        };
        (frame.id.clone(), sessions.visual.blocks.clone())
    };
    // An if-let scrutinee keeps its temporary MutexGuard throughout the body.
    // Read the Arc in a separate statement before ever writing the cache again.
    let image = { lock(frames).image(&frame_id)? };
    let mut blocks = snapshot.clone();
    let work = tauri::async_runtime::spawn_blocking(move || {
        let clean = screentrans_core::render::process_visual_rendering(&image, &mut blocks);
        Ok::<_, AppError>((RenderedImage::encode(clean)?, blocks))
    });
    let (rendered, blocks) = tokio::select! {
        _ = stop.cancelled() => return Err(AppError::cancelled()),
        result = work => result.map_err(|_| AppError::new(ErrorCode::Internal, "截图渲染失败，请重试"))??,
    };
    {
        let mut sessions = lock(sessions);
        let visual = &mut sessions.visual;
        if visual.revision == revision
            && !stop.is_cancelled()
            && visual.frame.as_ref().is_some_and(|f| f.id == frame_id)
            && same_content(&visual.blocks, &snapshot)
        {
            // Only the short cache insertion is serialized with the session commit.
            let clean_id = lock(frames).insert_derived(&frame_id, rendered)?;
            visual.blocks = blocks;
            visual.frame.as_mut().unwrap().clean_id = Some(clean_id);
        } else {
            return Err(AppError::cancelled());
        }
    }
    Ok(())
}

fn same_content(current: &[TextBlock], snapshot: &[TextBlock]) -> bool {
    current.len() == snapshot.len()
        && current.iter().zip(snapshot).all(|(a, b)| {
            a.id == b.id
                && a.revision == b.revision
                && a.text == b.text
                && a.translation == b.translation
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use screentrans_core::{CaptureFrame, OcrLine, Rect, TranslationSession, layout};
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[test]
    fn repeated_rendering_finishes_and_releases_cache_and_session_locks() {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let frames = Arc::new(Mutex::new(FrameCache::default()));
                let meta = lock(&frames)
                    .insert(
                        CaptureFrame {
                            image: image::RgbaImage::from_pixel(180, 80, image::Rgba([255; 4])),
                            screen_bounds: None,
                            scale_factor: 1.0,
                        },
                        &[],
                    )
                    .unwrap();
                let mut blocks = layout::group_lines(vec![OcrLine {
                    text: "Hello".into(),
                    bounds: Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 100.0,
                        height: 20.0,
                    },
                    confidence: None,
                }]);
                blocks[0].translation = "World".into();
                let sessions = Mutex::new(Sessions {
                    visual: TranslationSession {
                        id: "visual".into(),
                        revision: 1,
                        frame: Some(meta.clone()),
                        blocks,
                        ..Default::default()
                    },
                    ..Default::default()
                });
                for _ in 0..3 {
                    render(&frames, &sessions, 1, CancellationToken::new())
                        .await
                        .unwrap();
                    let clean_id = lock(&sessions)
                        .visual
                        .frame
                        .as_ref()
                        .unwrap()
                        .clean_id
                        .clone()
                        .unwrap();
                    assert!(lock(&frames).png(&clean_id).is_some());
                    assert!(lock(&frames).image(&meta.id).is_ok());
                }
                let stop = CancellationToken::new();
                stop.cancel();
                assert_eq!(
                    render(&frames, &sessions, 1, stop).await.unwrap_err().code,
                    ErrorCode::Cancelled
                );
                assert_eq!(
                    render(&frames, &sessions, 0, CancellationToken::new())
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::Cancelled
                );
            });
            sender.send(()).unwrap();
        });
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("rendering deadlocked; cache or session lock was retained");
    }
}
