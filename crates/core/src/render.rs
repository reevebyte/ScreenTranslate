use crate::{Rect, TextBlock};
use image::{Rgba, RgbaImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampledColors {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
}

pub fn luminance(color: [u8; 3]) -> f64 {
    0.2126 * color[0] as f64 + 0.7152 * color[1] as f64 + 0.0722 * color[2] as f64
}

fn linear_channel(val: u8) -> f64 {
    let c = val as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn relative_luminance(color: [u8; 3]) -> f64 {
    0.2126 * linear_channel(color[0])
        + 0.7152 * linear_channel(color[1])
        + 0.0722 * linear_channel(color[2])
}

pub fn contrast_ratio(c1: [u8; 3], c2: [u8; 3]) -> f64 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let lighter = l1.max(l2);
    let darker = l1.min(l2);
    (lighter + 0.05) / (darker + 0.05)
}

fn histogram_median(histogram: &[u32; 256], count: u64) -> u8 {
    let rank = count / 2;
    let mut seen = 0u64;
    for (value, &cnt) in histogram.iter().enumerate() {
        seen += cnt as u64;
        if seen > rank {
            return value as u8;
        }
    }
    0
}

pub fn sample_colors(image: &RgbaImage, rect: &Rect, line_height: f32) -> SampledColors {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return SampledColors {
            background: [255, 255, 255],
            foreground: [17, 17, 17],
        };
    }

    let padding = (line_height.max(1.0) * 0.35).round().max(2.0) as i32;
    let inner_left = (rect.x.floor() as i32).clamp(0, width);
    let inner_top = (rect.y.floor() as i32).clamp(0, height);
    let inner_right = (rect.right().ceil() as i32).clamp(inner_left, width);
    let inner_bottom = (rect.bottom().ceil() as i32).clamp(inner_top, height);

    let outer_left = (inner_left - padding).max(0);
    let outer_top = (inner_top - padding).max(0);
    let outer_right = (inner_right + padding).min(width);
    let outer_bottom = (inner_bottom + padding).min(height);

    let mut histograms = [[0u32; 256]; 3];
    let mut ring_count = 0u64;

    for y in outer_top..outer_bottom {
        for x in outer_left..outer_right {
            if x >= inner_left && x < inner_right && y >= inner_top && y < inner_bottom {
                continue;
            }
            let pixel = image.get_pixel(x as u32, y as u32);
            histograms[0][pixel[0] as usize] += 1;
            histograms[1][pixel[1] as usize] += 1;
            histograms[2][pixel[2] as usize] += 1;
            ring_count += 1;
        }
    }

    if ring_count < 24 {
        histograms = [[0u32; 256]; 3];
        ring_count = 0;
        for y in inner_top..inner_bottom {
            for x in inner_left..inner_right {
                let pixel = image.get_pixel(x as u32, y as u32);
                histograms[0][pixel[0] as usize] += 1;
                histograms[1][pixel[1] as usize] += 1;
                histograms[2][pixel[2] as usize] += 1;
                ring_count += 1;
            }
        }
    }

    let background = if ring_count > 0 {
        [
            histogram_median(&histograms[0], ring_count),
            histogram_median(&histograms[1], ring_count),
            histogram_median(&histograms[2], ring_count),
        ]
    } else {
        [255, 255, 255]
    };

    struct Candidate {
        dist: u32,
        r: u8,
        g: u8,
        b: u8,
    }
    let mut candidates = Vec::new();
    let bg_r = background[0] as i32;
    let bg_g = background[1] as i32;
    let bg_b = background[2] as i32;

    for y in inner_top..inner_bottom {
        for x in inner_left..inner_right {
            let pixel = image.get_pixel(x as u32, y as u32);
            let dr = pixel[0] as i32 - bg_r;
            let dg = pixel[1] as i32 - bg_g;
            let db = pixel[2] as i32 - bg_b;
            let dist = (dr * dr + dg * dg + db * db) as u32;
            if dist > 60 * 60 {
                candidates.push(Candidate {
                    dist,
                    r: pixel[0],
                    g: pixel[1],
                    b: pixel[2],
                });
            }
        }
    }

    let pixel_count = ((inner_right - inner_left) * (inner_bottom - inner_top)) as usize;
    let min_needed = 8.max(pixel_count / 100);
    let mut foreground = if luminance(background) > 127.0 {
        [17, 17, 17]
    } else {
        [240, 240, 240]
    };

    if candidates.len() >= min_needed {
        candidates.sort_unstable_by_key(|c| c.dist);
        let cutoff_index = candidates.len() * 3 / 5;
        let mut sum_r = 0u64;
        let mut sum_g = 0u64;
        let mut sum_b = 0u64;
        let mut count = 0u64;
        for c in &candidates[cutoff_index..] {
            sum_r += c.r as u64;
            sum_g += c.g as u64;
            sum_b += c.b as u64;
            count += 1;
        }
        if let [Some(r), Some(g), Some(b)] = [
            sum_r.checked_div(count),
            sum_g.checked_div(count),
            sum_b.checked_div(count),
        ] {
            foreground = [r as u8, g as u8, b as u8];
        }
    }

    if contrast_ratio(background, foreground) < 3.0 {
        foreground = if relative_luminance(background) > 0.45 {
            [17, 17, 17]
        } else {
            [240, 240, 240]
        };
    }

    SampledColors {
        background,
        foreground,
    }
}

pub fn repair_text_region(image: &mut RgbaImage, rect: &Rect, dark_text: bool, line_height: f32) {
    let img_w = image.width() as i32;
    let img_h = image.height() as i32;
    let x0 = (rect.x.floor() as i32).clamp(0, img_w);
    let y0 = (rect.y.floor() as i32).clamp(0, img_h);
    let x1 = (rect.right().ceil() as i32).clamp(x0, img_w);
    let y1 = (rect.bottom().ceil() as i32).clamp(y0, img_h);
    let width = (x1 - x0) as usize;
    let height = (y1 - y0) as usize;

    if width < 3 || height < 3 {
        return;
    }

    // Limit memory usage for enormous regions
    if width * height > 2_000_000 {
        return;
    }

    let patch_stride = width * 4;
    let mut patch = vec![0u8; patch_stride * height];
    let mut scratch = vec![0u8; patch_stride * height];

    for y in 0..height {
        for x in 0..width {
            let px = image.get_pixel((x0 as usize + x) as u32, (y0 as usize + y) as u32);
            let idx = y * patch_stride + x * 4;
            patch[idx..idx + 4].copy_from_slice(&px.0);
        }
    }

    let mut filter_size = ((line_height.max(1.0) * 0.34).round() as i32).clamp(3, 15);
    if filter_size % 2 == 0 {
        filter_size += 1;
    }
    let passes = (((height as i32 + 1) / 2).clamp(filter_size / 2, 16)).max(1) as usize;

    let choose = |a: u8, b: u8, c: u8| -> u8 {
        if dark_text {
            a.max(b).max(c)
        } else {
            a.min(b).min(c)
        }
    };

    for _ in 0..passes {
        // Horizontal min/max pass
        for y in 0..height {
            let row_offset = y * patch_stride;
            for x in 0..width {
                let prev_x = x.saturating_sub(1);
                let next_x = (x + 1).min(width - 1);
                let idx = row_offset + x * 4;
                let prev_idx = row_offset + prev_x * 4;
                let next_idx = row_offset + next_x * 4;
                for ch in 0..3 {
                    scratch[idx + ch] =
                        choose(patch[prev_idx + ch], patch[idx + ch], patch[next_idx + ch]);
                }
                scratch[idx + 3] = 255;
            }
        }
        // Vertical min/max pass
        for y in 0..height {
            let prev_y = y.saturating_sub(1);
            let next_y = (y + 1).min(height - 1);
            let row_offset = y * patch_stride;
            let prev_offset = prev_y * patch_stride;
            let next_offset = next_y * patch_stride;
            for x in 0..width {
                let x_offset = x * 4;
                let idx = row_offset + x_offset;
                let prev_idx = prev_offset + x_offset;
                let next_idx = next_offset + x_offset;
                for ch in 0..3 {
                    patch[idx + ch] = choose(
                        scratch[prev_idx + ch],
                        scratch[idx + ch],
                        scratch[next_idx + ch],
                    );
                }
                patch[idx + 3] = 255;
            }
        }
    }

    // Horizontal smoothing box blur
    for y in 0..height {
        let row_offset = y * patch_stride;
        for x in 0..width {
            let prev_x = x.saturating_sub(1);
            let next_x = (x + 1).min(width - 1);
            let idx = row_offset + x * 4;
            let prev_idx = row_offset + prev_x * 4;
            let next_idx = row_offset + next_x * 4;
            for ch in 0..3 {
                scratch[idx + ch] = ((patch[prev_idx + ch] as u16
                    + patch[idx + ch] as u16
                    + patch[next_idx + ch] as u16)
                    / 3) as u8;
            }
            scratch[idx + 3] = 255;
        }
    }

    // Vertical smoothing box blur + write back to image
    for y in 0..height {
        let prev_y = y.saturating_sub(1);
        let next_y = (y + 1).min(height - 1);
        let row_offset = y * patch_stride;
        let prev_offset = prev_y * patch_stride;
        let next_offset = next_y * patch_stride;
        for x in 0..width {
            let x_offset = x * 4;
            let prev_idx = prev_offset + x_offset;
            let idx = row_offset + x_offset;
            let next_idx = next_offset + x_offset;
            let mut final_px = [0u8; 4];
            for ch in 0..3 {
                final_px[ch] = ((scratch[prev_idx + ch] as u16
                    + scratch[idx + ch] as u16
                    + scratch[next_idx + ch] as u16)
                    / 3) as u8;
            }
            final_px[3] = 255;
            image.put_pixel(
                (x0 as usize + x) as u32,
                (y0 as usize + y) as u32,
                Rgba(final_px),
            );
        }
    }
}

pub fn fit_font_size(
    text: &str,
    target_width: f64,
    target_height: f64,
    line_height_hint: f64,
) -> (f32, f32) {
    let width = (target_width - 4.0).max(1.0);
    let height = (target_height - 2.0).max(1.0);
    let maximum = (line_height_hint * 1.08).clamp(8.0, 32.0);
    let line_height = 1.2;
    let mut size = maximum;
    while size > 6.0 {
        let lines: f64 = text
            .split('\n')
            .map(|line| {
                let units: f64 = line
                    .chars()
                    .map(|ch| {
                        if ch.is_whitespace() {
                            0.33
                        } else if ch.is_ascii() {
                            0.58
                        } else {
                            1.0
                        }
                    })
                    .sum();
                (units * size / width).ceil().max(1.0)
            })
            .sum();
        if lines * size * line_height <= height {
            break;
        }
        size -= 0.5;
    }
    (size.max(6.0) as f32, line_height as f32)
}

fn cover_source_line(image: &mut RgbaImage, rect: Rect, background: [u8; 3]) {
    let x0 = (rect.x.floor() - 1.0).clamp(0.0, image.width() as f64) as u32;
    let y0 = (rect.y.floor() - 1.0).clamp(0.0, image.height() as f64) as u32;
    let x1 = (rect.right().ceil() + 1.0).clamp(x0 as f64, image.width() as f64) as u32;
    let y1 = (rect.bottom().ceil() + 1.0).clamp(y0 as f64, image.height() as f64) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            image.put_pixel(
                x,
                y,
                Rgba([background[0], background[1], background[2], 255]),
            );
        }
    }
}

pub fn process_visual_rendering(image: &RgbaImage, blocks: &mut [TextBlock]) -> RgbaImage {
    let mut clean_image = image.clone();
    let source_bounds: Vec<_> = blocks
        .iter()
        .map(|b| crate::layout::union(b.source_lines.iter().copied()).unwrap_or(b.bounds))
        .collect();
    for (index, block) in blocks.iter_mut().enumerate() {
        let lines: Vec<_> = if block.source_lines.is_empty() {
            vec![block.bounds]
        } else {
            block.source_lines.clone()
        };
        let mut heights: Vec<_> = lines.iter().map(|line| line.height).collect();
        heights.sort_by(f64::total_cmp);
        let source_height = heights[heights.len() / 2].max(1.0);
        let source = source_bounds[index];
        let mut top_limit = 0.0_f64;
        let mut bottom_limit = image.height() as f64;
        for (other_index, other) in source_bounds.iter().enumerate() {
            if other_index == index || other.right() <= source.x || other.x >= source.right() {
                continue;
            }
            if other.bottom() <= source.y {
                top_limit = top_limit.max(other.bottom() + 1.0);
            }
            if other.y >= source.bottom() {
                bottom_limit = bottom_limit.min(other.y - 1.0);
            }
        }
        let top = (source.y - source_height * 0.2).max(top_limit);
        let bottom = (source.bottom() + source_height * 0.4).min(bottom_limit);
        // OCR bounds cover ink, not the font's ascenders, descenders and line box.
        block.bounds = Rect {
            x: source.x,
            y: top,
            width: source.width,
            height: (bottom - top).max(source.height),
        };
        let colors = sample_colors(image, &lines[0], source_height as f32);
        if !block.translation.trim().is_empty() {
            for line in &lines {
                let color = sample_colors(image, line, line.height as f32);
                cover_source_line(&mut clean_image, *line, color.background);
            }
        }
        let text = if block.translation.trim().is_empty() {
            &block.text
        } else {
            &block.translation
        };
        let mut right_limit = image.width() as f64 - 1.0;
        for (other_index, other) in source_bounds.iter().enumerate() {
            if other_index != index
                && other.y < source.bottom()
                && other.bottom() > source.y
                && other.x >= source.right()
            {
                right_limit = right_limit.min(other.x - 3.0);
            }
        }
        let desired_width = text
            .lines()
            .map(|line| {
                line.chars()
                    .map(|c| if c.is_ascii() { 0.58 } else { 1.0 })
                    .sum::<f64>()
                    * source_height
                    * 1.08
                    + 4.0
            })
            .fold(source.width, f64::max);
        let expanded_right = (source.x + desired_width)
            .min(right_limit)
            .max(source.right());
        let flat_extension = (0..12).all(|index| {
            let x = (source.right() + (expanded_right - source.right()) * index as f64 / 11.0)
                .clamp(0.0, image.width() as f64 - 1.0) as u32;
            [0.15, 0.5, 0.85].into_iter().all(|ratio| {
                let y = (source.y + source.height * ratio).clamp(0.0, image.height() as f64 - 1.0)
                    as u32;
                let pixel = image.get_pixel(x, y);
                (0..3).all(|channel| pixel[channel].abs_diff(colors.background[channel]) < 28)
            })
        });
        if flat_extension {
            block.bounds.width = expanded_right - source.x;
        }
        let (font_size, line_height) =
            fit_font_size(text, block.bounds.width, block.bounds.height, source_height);
        block.background_color = Some(format!(
            "#{:02x}{:02x}{:02x}",
            colors.background[0], colors.background[1], colors.background[2]
        ));
        block.foreground_color = Some(format!(
            "#{:02x}{:02x}{:02x}",
            colors.foreground[0], colors.foreground[1], colors.foreground[2]
        ));
        block.font_size = Some(font_size);
        block.line_height = Some(line_height);
    }
    clean_image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contrast_and_luminance() {
        let white = [255, 255, 255];
        let black = [0, 0, 0];
        let ratio = contrast_ratio(white, black);
        assert!(ratio > 20.0);
    }

    #[test]
    fn test_repair_text_region() {
        let mut img = RgbaImage::new(100, 100);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        // Draw a simulated dark text line
        for x in 20..80 {
            for y in 45..55 {
                img.put_pixel(x, y, Rgba([20, 20, 20, 255]));
            }
        }
        let bounds = Rect {
            x: 18.0,
            y: 43.0,
            width: 64.0,
            height: 14.0,
        };
        repair_text_region(&mut img, &bounds, true, 14.0);
        let center_pixel = img.get_pixel(50, 50);
        // Should have been repaired to near-white
        assert!(center_pixel[0] > 200);
        assert!(center_pixel[1] > 200);
        assert!(center_pixel[2] > 200);
    }

    #[test]
    fn test_font_fit() {
        let (size, lh) = fit_font_size("Hello World", 200.0, 30.0, 24.0);
        assert!(size >= 11.0);
        assert!(lh > 1.0);
    }

    #[test]
    fn wipes_source_glyphs_without_smearing_paragraph_gaps() {
        let mut image = RgbaImage::from_pixel(140, 60, Rgba([24, 24, 24, 255]));
        for y in [14..20, 36..42] {
            for row in y {
                for x in 15..95 {
                    image.put_pixel(x, row, Rgba([230; 4]));
                }
            }
        }
        image.put_pixel(20, 26, Rgba([200, 10, 10, 255]));
        let mut blocks = crate::layout::group_lines(
            [10.0, 28.0]
                .into_iter()
                .map(|y| crate::OcrLine {
                    text: "Source line".into(),
                    bounds: Rect {
                        x: 10.0,
                        y,
                        width: 100.0,
                        height: 14.0,
                    },
                    confidence: None,
                })
                .collect(),
        );
        assert_eq!(blocks.len(), 1);
        blocks[0].translation = "A translated paragraph that wraps onto several lines.".into();
        let clean = process_visual_rendering(&image, &mut blocks);
        assert_eq!(clean.get_pixel(30, 16), &Rgba([24, 24, 24, 255]));
        assert_eq!(clean.get_pixel(20, 26), image.get_pixel(20, 26));
        assert!(blocks[0].font_size.unwrap() <= 14.0);
        blocks[0].translation.clear();
        assert_eq!(process_visual_rendering(&image, &mut blocks), image);
    }
}
