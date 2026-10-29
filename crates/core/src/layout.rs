use crate::{OcrLine, Rect, TextBlock};

pub fn group_lines(mut lines: Vec<OcrLine>) -> Vec<TextBlock> {
    for line in &mut lines {
        line.text = normalize_ocr_text(&line.text);
    }
    lines.retain(|line| !line.text.trim().is_empty() && line.bounds.validate().is_ok());
    lines.sort_by(|a, b| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
    });
    let mut blocks: Vec<TextBlock> = Vec::new();
    for line in lines {
        let candidate = blocks.iter_mut().rev().find(|block| {
            let source_height = block
                .source_lines
                .last()
                .map_or(block.bounds.height, |r| r.height);
            let gap = line.bounds.y - block.bounds.bottom();
            let horizontal_overlap =
                line.bounds.right().min(block.bounds.right()) - line.bounds.x.max(block.bounds.x);
            !list_item(&line.text)
                && !block.text.lines().last().is_some_and(list_item)
                && (0.7..=1.4).contains(&(line.bounds.height / source_height))
                && gap >= -line.bounds.height * 0.3
                && gap <= line.bounds.height * 0.45
                && horizontal_overlap > line.bounds.width.min(block.bounds.width) * 0.6
                && (line.bounds.x - block.bounds.x).abs() < line.bounds.height * 1.8
        });
        if let Some(block) = candidate {
            let right = block.bounds.right().max(line.bounds.right());
            let bottom = block.bounds.bottom().max(line.bounds.bottom());
            block.bounds.x = block.bounds.x.min(line.bounds.x);
            block.bounds.y = block.bounds.y.min(line.bounds.y);
            block.bounds.width = right - block.bounds.x;
            block.bounds.height = bottom - block.bounds.y;
            block.source_lines.push(line.bounds);
            block.text.push('\n');
            block.text.push_str(line.text.trim());
            block.confidence = match (block.confidence, line.confidence) {
                (Some(a), Some(b)) => Some(a.min(b)),
                _ => None,
            };
        } else {
            blocks.push(TextBlock {
                id: format!("block-{}", blocks.len()),
                bounds: line.bounds,
                source_lines: vec![line.bounds],
                text: line.text.trim().into(),
                translation: String::new(),
                target_language: String::new(),
                confidence: line.confidence,
                revision: 0,
                background_color: None,
                foreground_color: None,
                font_size: None,
                line_height: None,
            });
        }
    }
    blocks.sort_by(|a, b| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
    });
    blocks
}

pub fn normalize_ocr_text(text: &str) -> String {
    let cjk = |c: char| matches!(c as u32, 0x2e80..=0x9fff | 0xac00..=0xd7af | 0x20000..=0x2fa1f);
    let punctuation = |c: char| matches!(c as u32, 0x3000..=0x303f | 0xff01..=0xff0f | 0xff1a..=0xff20 | 0xff3b..=0xff40 | 0xff5b..=0xff65);
    let mut result = String::with_capacity(text.len());
    let mut previous = None;
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space
            && let Some(prev) = previous
            && !(cjk(prev) && cjk(c) || punctuation(prev) || punctuation(c))
        {
            result.push(' ');
        }
        result.push(c);
        previous = Some(c);
        space = false;
    }
    result
}

fn list_item(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with(['•', '●', '▪', '·', '-', '–', '—'])
        || text
            .split_once(['.', ')', '、'])
            .is_some_and(|(prefix, _)| {
                !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit())
            })
}

pub fn union(rects: impl Iterator<Item = Rect>) -> Option<Rect> {
    rects.reduce(|a, b| {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Rect {
            x,
            y,
            width: a.right().max(b.right()) - x,
            height: a.bottom().max(b.bottom()) - y,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f64, y: f64) -> OcrLine {
        OcrLine {
            text: text.into(),
            bounds: Rect {
                x,
                y,
                width: 100.0,
                height: 20.0,
            },
            confidence: None,
        }
    }

    #[test]
    fn merges_paragraphs_but_preserves_columns() {
        let blocks = group_lines(vec![
            line("First", 0.0, 0.0),
            line("Second", 0.0, 25.0),
            line("Other", 250.0, 0.0),
        ]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "First\nSecond");
        assert_eq!(blocks[0].source_lines.len(), 2);
        assert_eq!(blocks[1].text, "Other");
    }

    #[test]
    fn keeps_bullets_and_headings_separate() {
        let mut heading = line("Heading", 0.0, 0.0);
        heading.bounds.height = 32.0;
        let blocks = group_lines(vec![
            heading,
            line("• One", 0.0, 38.0),
            line("• Two", 0.0, 63.0),
        ]);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].text, "• One");
        assert_eq!(blocks[2].source_lines[0].y, 63.0);
    }

    #[test]
    fn removes_ocr_character_spacing_without_joining_english_words() {
        assert_eq!(
            normalize_ocr_text("去 掉 顶 部 标 志 ， 改 为 黑 白 文 字 。"),
            "去掉顶部标志，改为黑白文字。"
        );
        assert_eq!(
            normalize_ocr_text("已 构 建 alpha.4 ．"),
            "已构建 alpha.4．"
        );
        assert_eq!(
            normalize_ocr_text("Hello   world from OCR"),
            "Hello world from OCR"
        );
    }
}
