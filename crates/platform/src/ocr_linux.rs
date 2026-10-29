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
    let requested: Vec<_> = requested
        .iter()
        .map(|code| match code.as_str() {
            "zh-Hans-CN" | "zh-Hans" | "zh-CN" => "chi_sim",
            "zh-Hant" | "zh-TW" => "chi_tra",
            "en-US" | "en" => "eng",
            "ja-JP" | "ja" => "jpn",
            "ko-KR" | "ko" => "kor",
            "fr-FR" | "fr" => "fra",
            "de-DE" | "de" => "deu",
            "es-ES" | "es" => "spa",
            "ru-RU" | "ru" => "rus",
            other => other,
        })
        .filter(|code| installed.iter().any(|installed| installed == code))
        .collect();
    if requested.is_empty() {
        return Err(unavailable());
    }
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| unavailable())?;
    // No shell expansion; language names are restricted to the installed engine's own list.
    let mut child = tokio::process::Command::new("tesseract")
        .args(["stdin", "stdout", "-l", &requested.join("+"), "tsv"])
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
        "请安装 Tesseract 5 及所选语言包：tesseract-ocr-eng、tesseract-ocr-chi-sim",
    )
    .action("ocr")
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
            Some(OcrLine {
                bounds,
                text: words
                    .iter()
                    .map(|(_, text, _)| text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
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
}
