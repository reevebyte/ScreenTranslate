use image::RgbaImage;
use screentrans_core::{AppError, ErrorCode, OcrLine, Rect, Result};
use std::{collections::BTreeMap, io::Cursor, sync::Arc};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

pub fn languages() -> (bool, Vec<String>) {
    let output = std::process::Command::new("tesseract")
        .arg("--list-langs")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let languages: Vec<_> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .skip(1)
                .filter(|line| !line.trim().is_empty() && *line != "osd")
                .map(str::to_owned)
                .collect();
            (!languages.is_empty(), languages)
        }
        _ => (false, Vec::new()),
    }
}

pub async fn recognize(
    image: Arc<RgbaImage>,
    requested: Vec<String>,
    stop: CancellationToken,
) -> Result<Vec<OcrLine>> {
    let installed = tokio::task::spawn_blocking(languages)
        .await
        .map_err(|_| unavailable())?
        .1;
    let mapped: Vec<_> = requested
        .iter()
        .map(|code| match code.as_str() {
            "zh-Hans-CN" | "zh-Hans" | "zh-CN" => "chi_sim",
            "zh-Hant" | "zh-Hant-TW" | "zh-Hant-HK" | "zh-TW" => "chi_tra",
            "en-US" | "en" => "eng",
            "ja-JP" | "ja" => "jpn",
            "ko-KR" | "ko" => "kor",
            "fr-FR" | "fr" => "fra",
            "de-DE" | "de" => "deu",
            "es-ES" | "es" => "spa",
            "ru-RU" | "ru" => "rus",
            other => other,
        })
        .collect();
    let available: Vec<_> = mapped
        .iter()
        .copied()
        .filter(|code| installed.iter().any(|installed| installed == code))
        .collect();
    if available.is_empty() {
        return Err(unavailable_for(&mapped, &installed));
    }
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| unavailable())?;
    // No shell expansion; language names are restricted to the installed engine's own list.
    let mut child = tokio::process::Command::new("tesseract")
        .args(["stdin", "stdout", "-l", &available.join("+"), "tsv"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| unavailable())?;
    let mut stdin = child.stdin.take().ok_or_else(unavailable)?;
    let bytes = png.into_inner();
    tokio::spawn(async move {
        let _ = stdin.write_all(&bytes).await;
    });
    let result = tokio::select! {
        _ = stop.cancelled() => return Err(AppError::cancelled()),
        result = tokio::time::timeout(std::time::Duration::from_secs(45), child.wait_with_output()) =>
            result.map_err(|_| unavailable())?.map_err(|_| unavailable())?,
    };
    if !result.status.success() {
        return Err(unavailable());
    }
    parse_tsv(&String::from_utf8_lossy(&result.stdout))
}

fn unavailable() -> AppError {
    AppError::new(
        ErrorCode::OcrUnavailable,
        "请安装 Tesseract 5 及所需语言包（如 eng、chi_sim）",
    )
    .action("ocr")
}

fn unavailable_for(requested: &[&str], installed: &[String]) -> AppError {
    if installed.is_empty() || requested.is_empty() {
        return unavailable();
    }
    let missing: Vec<_> = requested
        .iter()
        .copied()
        .filter(|code| !installed.iter().any(|item| item == code))
        .collect();
    if missing.is_empty() {
        return unavailable();
    }
    AppError::new(
        ErrorCode::OcrUnavailable,
        format!(
            "未找到所需 Tesseract 语言包（{}）；请安装对应语言数据",
            missing.join("、")
        ),
    )
    .action("ocr")
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3000}'..='\u{303F}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
    )
}

fn is_cjk_punctuation(ch: char) -> bool {
    matches!(ch, '\u{3000}'..='\u{303F}' | '\u{FF01}'..='\u{FF0F}' | '\u{FF1A}'..='\u{FF20}' | '\u{FF3B}'..='\u{FF40}' | '\u{FF5B}'..='\u{FF65}')
}

fn join_ocr_words(words: &[&str]) -> String {
    let mut joined = String::new();
    for word in words {
        if word.is_empty() {
            continue;
        }
        if let (Some(prev), Some(next)) = (joined.chars().last(), word.chars().next()) {
            let omit_space = (is_cjk_char(prev) && is_cjk_char(next))
                || is_cjk_punctuation(prev)
                || is_cjk_punctuation(next);
            if !omit_space {
                joined.push(' ');
            }
        }
        joined.push_str(word);
    }
    joined
}

fn parse_tsv(tsv: &str) -> Result<Vec<OcrLine>> {
    type LineKey = (String, String, String, String);
    type Word = (Rect, String, f32);
    let mut groups: BTreeMap<LineKey, Vec<Word>> = BTreeMap::new();
    for row in tsv.lines().skip(1) {
        let values: Vec<_> = row.splitn(12, '\t').collect();
        if values.len() != 12 || values[0] != "5" || values[11].trim().is_empty() {
            continue;
        }
        let parse = |i: usize| values[i].parse::<f64>().ok();
        let (Some(x), Some(y), Some(width), Some(height)) =
            (parse(6), parse(7), parse(8), parse(9))
        else {
            continue;
        };
        let key = (
            values[1].into(),
            values[2].into(),
            values[3].into(),
            values[4].into(),
        );
        groups.entry(key).or_default().push((
            Rect {
                x,
                y,
                width,
                height,
            },
            values[11].into(),
            values[10].parse::<f32>().unwrap_or(0.0) / 100.0,
        ));
    }
    Ok(groups
        .into_values()
        .filter_map(|words| {
            let bounds = screentrans_core::layout::union(words.iter().map(|(rect, _, _)| *rect))?;
            let tokens: Vec<_> = words.iter().map(|(_, text, _)| text.as_str()).collect();
            Some(OcrLine {
                bounds,
                text: join_ocr_words(&tokens),
                confidence: Some(
                    words
                        .iter()
                        .map(|(_, _, confidence)| *confidence)
                        .sum::<f32>()
                        / words.len() as f32,
                ),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_tsv_words_without_losing_geometry() {
        let result = parse_tsv("level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t10\t20\t40\t15\t95\tHello\n5\t1\t1\t1\t1\t2\t55\t20\t40\t15\t90\tworld").unwrap();
        assert_eq!(result[0].text, "Hello world");
        assert_eq!(result[0].bounds.width, 85.0);
    }

    #[test]
    fn joins_cjk_words_without_spurious_spaces_and_keeps_latin_spaces() {
        assert_eq!(
            join_ocr_words(&["你", "好", "，", "世", "界"]),
            "你好，世界"
        );
        assert_eq!(
            join_ocr_words(&["使", "用", "ScreenTranslate", "进", "行", "翻", "译"]),
            "使用 ScreenTranslate 进行翻译"
        );
        assert_eq!(join_ocr_words(&["Hello", "world"]), "Hello world");
        assert_eq!(
            join_ocr_words(&["测试", "（", "Linux", "）"]),
            "测试（Linux）"
        );
    }

    #[test]
    fn unavailable_error_names_missing_tesseract_languages() {
        let err = unavailable_for(&["chi_sim", "eng"], &["eng".into()]);
        assert!(err.message.contains("chi_sim"));
        assert!(!err.message.contains("eng"));
    }
}
