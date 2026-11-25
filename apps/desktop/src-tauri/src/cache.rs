use image::{
    ExtendedColorType, ImageEncoder, RgbaImage,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use screentrans_core::{AppError, CaptureFrame, ErrorCode, FrameMetadata, Rect, Result};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

const CACHE_LIMIT: usize = 192 * 1024 * 1024;

pub struct RenderedImage {
    png: Vec<u8>,
}

impl RenderedImage {
    pub fn encode(image: RgbaImage) -> Result<Self> {
        Ok(Self {
            png: encode_png(&image)?,
        })
    }
}

#[derive(Clone, Copy)]
enum PreviewFormat {
    Png,
}

impl PreviewFormat {
    fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
        }
    }
}

pub struct CachedFrame {
    pub metadata: FrameMetadata,
    pub image: Option<Arc<RgbaImage>>,
    preview: Arc<Vec<u8>>,
    preview_format: PreviewFormat,
    is_source: bool,
}

#[derive(Default)]
pub struct FrameCache {
    sequence: u64,
    bytes: usize,
    frames: HashMap<String, CachedFrame>,
    order: VecDeque<String>,
}

pub struct PreparedFrame {
    frame: CaptureFrame,
    preview: Vec<u8>,
    preview_format: PreviewFormat,
    cost: usize,
}

impl FrameCache {
    /// Prepares a fullscreen capture for selection. On Windows, the native Win32
    /// selection overlay reads the raw `RgbaImage` directly, skipping the
    /// multi-megabyte BMP preview encoding used only by WebView overlays.
    pub fn prepare_selection(frame: CaptureFrame) -> Result<PreparedFrame> {
        #[cfg(windows)]
        {
            if frame.image.width() == 0
                || frame.image.height() == 0
                || frame.image.as_raw().len() > CACHE_LIMIT
            {
                return Err(AppError::new(
                    ErrorCode::InvalidInput,
                    "图像过大或为空，请缩小截图范围",
                ));
            }
            let cost = frame.image.as_raw().len();
            Ok(PreparedFrame {
                frame,
                preview: Vec::new(),
                preview_format: PreviewFormat::Png,
                cost,
            })
        }
        #[cfg(not(windows))]
        {
            Self::prepare(frame)
        }
    }

    pub fn prepare(frame: CaptureFrame) -> Result<PreparedFrame> {
        if frame.image.width() == 0
            || frame.image.height() == 0
            || frame.image.as_raw().len() > CACHE_LIMIT
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "图像过大或为空，请缩小截图范围",
            ));
        }
        let (preview, preview_format) = encode_screen_preview(&frame.image)?;
        let cost = preview.len() + frame.image.as_raw().len();
        if cost > CACHE_LIMIT {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "截图超过缓存上限，请选择更小的区域",
            ));
        }
        Ok(PreparedFrame {
            frame,
            preview,
            preview_format,
            cost,
        })
    }

    #[cfg(test)]
    pub fn insert(&mut self, frame: CaptureFrame, protected: &[String]) -> Result<FrameMetadata> {
        let prepared = Self::prepare(frame)?;
        self.insert_prepared(prepared, protected)
    }

    pub fn insert_prepared(
        &mut self,
        prepared: PreparedFrame,
        protected: &[String],
    ) -> Result<FrameMetadata> {
        let PreparedFrame {
            frame,
            preview,
            preview_format,
            cost,
        } = prepared;
        let candidates: Vec<_> = self
            .order
            .iter()
            .filter(|id| !protected.contains(id))
            .cloned()
            .collect();
        for id in candidates {
            if self.bytes + cost <= CACHE_LIMIT {
                break;
            }
            self.remove(&id);
        }
        if self.bytes + cost > CACHE_LIMIT {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "多个显示器图像超过缓存上限，请指定单个显示器截图",
            ));
        }
        self.sequence += 1;
        let metadata = FrameMetadata {
            id: format!("frame-{}", self.sequence),
            width: frame.image.width(),
            height: frame.image.height(),
            screen_bounds: frame.screen_bounds,
            scale_factor: frame.scale_factor,
            clean_id: None,
        };
        self.frames.insert(
            metadata.id.clone(),
            CachedFrame {
                metadata: metadata.clone(),
                image: Some(Arc::new(frame.image)),
                preview: Arc::new(preview),
                preview_format,
                is_source: true,
            },
        );
        self.order.push_back(metadata.id.clone());
        self.bytes += cost;
        Ok(metadata)
    }

    pub fn insert_derived(&mut self, parent_id: &str, rendered: RenderedImage) -> Result<String> {
        let metadata = self.metadata(parent_id)?;
        let RenderedImage { png } = rendered;
        let preview = Arc::new(png);
        let cost = preview.len();
        if cost > CACHE_LIMIT {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "截图超过缓存上限，请缩小选区",
            ));
        }
        let old_id = self
            .frames
            .get(parent_id)
            .and_then(|frame| frame.metadata.clean_id.clone());
        let old_cost = old_id
            .as_ref()
            .and_then(|id| self.frames.get(id))
            .map_or(0, |frame| {
                frame.preview.len() + frame.image.as_ref().map_or(0, |image| image.as_raw().len())
            });
        if self.bytes - old_cost + cost > CACHE_LIMIT {
            let candidates: Vec<_> = self
                .order
                .iter()
                .filter(|id| id.as_str() != parent_id && old_id.as_deref() != Some(id.as_str()))
                .cloned()
                .collect();
            for id in candidates {
                if self.bytes - old_cost + cost <= CACHE_LIMIT {
                    break;
                }
                self.remove(&id);
            }
        }
        if self.bytes - old_cost + cost > CACHE_LIMIT {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "截图超过缓存上限，请缩小选区",
            ));
        }
        if let Some(old_id) = old_id {
            self.remove(&old_id);
        }
        self.sequence += 1;
        let derived_id = format!("{parent_id}-clean-{}", self.sequence);
        self.frames.insert(
            derived_id.clone(),
            CachedFrame {
                metadata: FrameMetadata {
                    id: derived_id.clone(),
                    width: metadata.width,
                    height: metadata.height,
                    screen_bounds: None,
                    scale_factor: 1.0,
                    clean_id: None,
                },
                image: None,
                preview,
                preview_format: PreviewFormat::Png,
                is_source: false,
            },
        );
        self.order.push_back(derived_id.clone());
        self.bytes += cost;
        if let Some(parent) = self.frames.get_mut(parent_id) {
            parent.metadata.clean_id = Some(derived_id.clone());
        }
        Ok(derived_id)
    }

    pub fn image(&self, id: &str) -> Result<Arc<RgbaImage>> {
        let frame = self.frames.get(id).ok_or_else(missing)?;
        if let Some(image) = &frame.image {
            return Ok(Arc::clone(image));
        }
        if !frame.is_source || frame.preview.is_empty() {
            return Err(missing());
        }
        let decoded = image::load_from_memory(&frame.preview)
            .map_err(|_| AppError::new(ErrorCode::Internal, "截图解码失败，请重新截图"))?;
        Ok(Arc::new(decoded.to_rgba8()))
    }
    /// Drops raw uncompressed RGBA bitmaps for source frames that already have a
    /// compressed PNG preview, keeping instant preview/toggle intact while
    /// reclaiming tens of megabytes when idle in the tray.
    pub fn release_idle_raw_images(&mut self) -> bool {
        let mut released = false;
        for frame in self.frames.values_mut() {
            if frame.is_source
                && !frame.preview.is_empty()
                && let Some(raw) = frame.image.take()
            {
                self.bytes = self.bytes.saturating_sub(raw.as_raw().len());
                released = true;
            }
        }
        if released {
            crate::windows::trim_linux_heap();
        }
        released
    }
    pub fn preview(&self, id: &str) -> Option<(Arc<Vec<u8>>, &'static str)> {
        self.frames
            .get(id)
            .map(|frame| (frame.preview.clone(), frame.preview_format.content_type()))
    }
    #[cfg(test)]
    pub fn png(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        self.frames.get(id).and_then(|frame| {
            matches!(frame.preview_format, PreviewFormat::Png).then(|| frame.preview.clone())
        })
    }
    pub fn metadata(&self, id: &str) -> Result<FrameMetadata> {
        self.frames
            .get(id)
            .map(|frame| frame.metadata.clone())
            .ok_or_else(missing)
    }
    pub fn remove(&mut self, id: &str) {
        if let Some(frame) = self.frames.remove(id) {
            self.bytes -=
                frame.preview.len() + frame.image.as_ref().map_or(0, |image| image.as_raw().len());
        }
        self.order.retain(|entry| entry != id);
    }
    /// Release obsolete captures only after their replacement has been presented.
    pub fn retain_current(&mut self, id: &str) {
        // A newer capture may already have entered the cache while this callback was queued.
        // Only evict predecessors of the presented frame; never discard newer, unpresented data.
        let Some(index) = self.order.iter().position(|key| key == id) else {
            return;
        };
        let stale: Vec<_> = self.order.iter().take(index).cloned().collect();
        let evicted = !stale.is_empty();
        for key in stale {
            self.remove(&key);
        }
        if evicted {
            crate::windows::trim_linux_heap();
        }
    }
    pub fn crop(&self, id: &str, selection: Rect) -> Result<CaptureFrame> {
        let cached = self.frames.get(id).ok_or_else(missing)?;
        let image = self.image(id)?;
        let (x, y, width, height) = selection.crop_bounds(image.width(), image.height())?;
        let bounds = cached.metadata.screen_bounds.map(|bounds| Rect {
            x: bounds.x + x as f64 * bounds.width / cached.metadata.width as f64,
            y: bounds.y + y as f64 * bounds.height / cached.metadata.height as f64,
            width: width as f64 * bounds.width / cached.metadata.width as f64,
            height: height as f64 * bounds.height / cached.metadata.height as f64,
        });
        Ok(CaptureFrame {
            image: image::imageops::crop_imm(image.as_ref(), x, y, width, height).to_image(),
            screen_bounds: bounds,
            scale_factor: cached.metadata.scale_factor,
        })
    }
}

fn missing() -> AppError {
    AppError::new(ErrorCode::InvalidInput, "截图已释放，请重新截图")
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|_| AppError::new(ErrorCode::Internal, "图像编码失败"))?;
    Ok(png)
}

fn encode_screen_preview(image: &RgbaImage) -> Result<(Vec<u8>, PreviewFormat)> {
    Ok((encode_png(image)?, PreviewFormat::Png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_raw_image_release_frees_rgba_bytes_and_decodes_on_demand() {
        let mut cache = FrameCache::default();
        let source = RgbaImage::from_fn(16, 12, |x, y| {
            image::Rgba([(x * 10) as u8, (y * 15) as u8, 120, 255])
        });
        let frame = cache
            .insert(
                CaptureFrame {
                    image: source.clone(),
                    screen_bounds: None,
                    scale_factor: 1.0,
                },
                &[],
            )
            .unwrap();
        let before_bytes = cache.bytes;
        assert!(cache.release_idle_raw_images());
        assert!(cache.bytes < before_bytes);
        assert!(!cache.release_idle_raw_images());
        let restored = cache.image(&frame.id).unwrap();
        assert_eq!(restored.dimensions(), source.dimensions());
        assert_eq!(restored.get_pixel(3, 4), source.get_pixel(3, 4));
    }

    #[test]
    fn repeated_rendering_replaces_previous_image_without_growing_cache() {
        let mut cache = FrameCache::default();
        let image = RgbaImage::new(32, 32);
        let meta = cache
            .insert(
                CaptureFrame {
                    image: image.clone(),
                    screen_bounds: None,
                    scale_factor: 1.0,
                },
                &[],
            )
            .unwrap();
        let mut previous = cache
            .insert_derived(&meta.id, RenderedImage::encode(image.clone()).unwrap())
            .unwrap();
        let bytes = cache.bytes;
        for _ in 0..4 {
            let next = cache
                .insert_derived(&meta.id, RenderedImage::encode(image.clone()).unwrap())
                .unwrap();
            assert_ne!(previous, next);
            assert!(cache.png(&previous).is_none());
            assert_eq!(cache.bytes, bytes);
            assert_eq!(cache.order.len(), 2);
            assert!(cache.image(&meta.id).is_ok());
            previous = next;
        }
    }
    #[test]
    fn presented_replacements_release_older_sources_and_clean_layers() {
        let mut cache = FrameCache::default();
        for _ in 0..20 {
            let image = RgbaImage::new(640, 360);
            let frame = cache
                .insert(
                    CaptureFrame {
                        image: image.clone(),
                        screen_bounds: None,
                        scale_factor: 1.0,
                    },
                    &[],
                )
                .unwrap();
            let clean = cache
                .insert_derived(&frame.id, RenderedImage::encode(image).unwrap())
                .unwrap();
            cache.retain_current(&frame.id);
            assert_eq!(cache.frames.len(), 2);
            assert!(cache.png(&clean).is_some());
            assert!(cache.image(&frame.id).is_ok());
            assert!(cache.image(&clean).is_err());
            let (frame_preview, _) = cache.preview(&frame.id).unwrap();
            assert_eq!(
                cache.bytes,
                640 * 360 * 4 + frame_preview.len() + cache.png(&clean).unwrap().len()
            );
        }
    }
    #[test]
    fn late_presentation_does_not_remove_a_newer_unpresented_capture() {
        let mut cache = FrameCache::default();
        let frame = || CaptureFrame {
            image: RgbaImage::new(10, 10),
            screen_bounds: None,
            scale_factor: 1.0,
        };
        let shown = cache.insert(frame(), &[]).unwrap();
        let next = cache.insert(frame(), &[]).unwrap();
        cache.retain_current(&shown.id);
        assert!(cache.image(&shown.id).is_ok());
        assert!(cache.image(&next.id).is_ok());
        cache.retain_current(&next.id);
        assert!(cache.image(&shown.id).is_err());
        assert!(cache.image(&next.id).is_ok());
    }
    #[test]
    fn crop_keeps_negative_screen_origin_and_scale() {
        let mut cache = FrameCache::default();
        let meta = cache
            .insert(
                CaptureFrame {
                    image: RgbaImage::new(100, 100),
                    scale_factor: 2.0,
                    screen_bounds: Some(Rect {
                        x: -2000.0,
                        y: 100.0,
                        width: 100.0,
                        height: 100.0,
                    }),
                },
                &[],
            )
            .unwrap();
        let crop = cache
            .crop(
                &meta.id,
                Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
            )
            .unwrap();
        assert_eq!(crop.screen_bounds.unwrap().x, -1990.0);
        assert_eq!(crop.image.width(), 30);
        assert_eq!(crop.scale_factor, 2.0);
    }

    #[test]
    fn retina_pixels_map_back_to_desktop_points() {
        let mut cache = FrameCache::default();
        let meta = cache
            .insert(
                CaptureFrame {
                    image: RgbaImage::new(200, 200),
                    scale_factor: 2.0,
                    screen_bounds: Some(Rect {
                        x: -100.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    }),
                },
                &[],
            )
            .unwrap();
        let crop = cache
            .crop(
                &meta.id,
                Rect {
                    x: 20.0,
                    y: 40.0,
                    width: 80.0,
                    height: 60.0,
                },
            )
            .unwrap();
        let bounds = crop.screen_bounds.unwrap();
        assert_eq!(bounds.x, -90.0);
        assert_eq!(bounds.y, 20.0);
        assert_eq!(bounds.width, 40.0);
        assert_eq!(crop.image.width(), 80);
    }
}
