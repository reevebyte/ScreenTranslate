use image::RgbaImage;
use screentrans_core::{AppError, CaptureFrame, ErrorCode, FrameMetadata, Rect, Result};
use std::{
    collections::{HashMap, VecDeque},
    io::Cursor,
    sync::Arc,
};

const CACHE_LIMIT: usize = 192 * 1024 * 1024;

pub struct RenderedImage {
    png: Vec<u8>,
}

impl RenderedImage {
    pub fn encode(image: RgbaImage) -> Result<Self> {
        let mut cursor = Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|_| AppError::new(ErrorCode::Internal, "图像编码失败"))?;
        Ok(Self {
            png: cursor.into_inner(),
        })
    }
}

pub struct CachedFrame {
    pub metadata: FrameMetadata,
    pub image: Option<Arc<RgbaImage>>,
    pub png: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct FrameCache {
    sequence: u64,
    bytes: usize,
    frames: HashMap<String, CachedFrame>,
    order: VecDeque<String>,
}

impl FrameCache {
    pub fn insert(&mut self, frame: CaptureFrame, protected: &[String]) -> Result<FrameMetadata> {
        if frame.image.width() == 0
            || frame.image.height() == 0
            || frame.image.as_raw().len() > CACHE_LIMIT
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "图像过大或为空，请缩小截图范围",
            ));
        }
        let mut cursor = Cursor::new(Vec::new());
        frame
            .image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|_| AppError::new(ErrorCode::Internal, "图像编码失败"))?;
        let png = cursor.into_inner();
        let cost = png.len() + frame.image.as_raw().len();
        if cost > CACHE_LIMIT {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "截图超过缓存上限，请选择更小的区域",
            ));
        }
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
                png: Arc::new(png),
            },
        );
        self.order.push_back(metadata.id.clone());
        self.bytes += cost;
        Ok(metadata)
    }

    pub fn insert_derived(&mut self, parent_id: &str, rendered: RenderedImage) -> Result<String> {
        let metadata = self.metadata(parent_id)?;
        let RenderedImage { png } = rendered;
        let cost = png.len();
        let old_id = self
            .frames
            .get(parent_id)
            .and_then(|frame| frame.metadata.clean_id.clone());
        let old_cost = old_id
            .as_ref()
            .and_then(|id| self.frames.get(id))
            .map_or(0, |frame| {
                frame.png.len() + frame.image.as_ref().map_or(0, |image| image.as_raw().len())
            });
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
                png: Arc::new(png),
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
        self.frames
            .get(id)
            .and_then(|frame| frame.image.clone())
            .ok_or_else(missing)
    }
    pub fn png(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        self.frames.get(id).map(|frame| frame.png.clone())
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
                frame.png.len() + frame.image.as_ref().map_or(0, |image| image.as_raw().len());
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
        for key in stale {
            self.remove(&key);
        }
    }
    pub fn crop(&self, id: &str, selection: Rect) -> Result<CaptureFrame> {
        let cached = self.frames.get(id).ok_or_else(missing)?;
        let image = cached.image.as_ref().ok_or_else(missing)?;
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

#[cfg(test)]
mod tests {
    use super::*;
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
            assert_eq!(
                cache.bytes,
                640 * 360 * 4
                    + cache.png(&frame.id).unwrap().len()
                    + cache.png(&clean).unwrap().len()
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
