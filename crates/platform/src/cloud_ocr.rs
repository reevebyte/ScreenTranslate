use async_trait::async_trait;
use image::RgbaImage;
use screentrans_core::{
    AppError, ErrorCode, OcrLine, Rect, Result, config::validate_api_url, ports::OcrBackend,
    translators::json_response,
};
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

pub struct AzureOcr {
    pub endpoint: String,
    pub key: String,
}

pub struct YoudaoOcr;

fn youdao_signature(png: &[u8], salt: &str) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    let short = format!(
        "{}{}{}",
        &encoded[..encoded.len().min(10)],
        encoded.len(),
        &encoded[encoded.len().saturating_sub(10)..]
    );
    format!(
        "{:x}",
        md5::compute(format!("deskdict{short}{salt}VPaHE3kX_vl4BhgYiu2n"))
    )
}

fn youdao_bounds(value: &serde_json::Value) -> Option<Rect> {
    let values: Vec<f64> = value
        .as_str()?
        .split(',')
        .map(str::trim)
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    if values.len() != 4
        || !values.iter().all(|v| v.is_finite())
        || values[2] <= 0.0
        || values[3] <= 0.0
    {
        return None;
    }
    Some(Rect {
        x: values[0],
        y: values[1],
        width: values[2],
        height: values[3],
    })
}

fn parse_youdao(value: &serde_json::Value) -> Result<Vec<OcrLine>> {
    let invalid = || {
        AppError::new(
            ErrorCode::InvalidResponse,
            "有道 OCR 返回格式异常，请重试或切换识别引擎",
        )
        .action("ocr")
    };
    if value["errorCode"] != "0" && value["errorCode"] != 0 {
        return Err(invalid());
    }
    let regions = value["resRegions"].as_array().ok_or_else(invalid)?;
    let mut output = Vec::new();
    for region in regions {
        let bounds = youdao_bounds(&region["boundingBox"]).ok_or_else(invalid)?;
        let mut lines: Vec<(String, Option<Rect>)> = region["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|line| {
                let text = line["text"].as_str()?.trim();
                (!text.is_empty()).then(|| (text.to_owned(), youdao_bounds(&line["boundingBox"])))
            })
            .collect();
        if lines.is_empty() {
            lines = region["context"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| (v.to_owned(), None))
                .collect();
        }
        let height = bounds.height / lines.len().max(1) as f64;
        for (index, (text, line_bounds)) in lines.into_iter().enumerate() {
            output.push(OcrLine {
                text,
                bounds: line_bounds.unwrap_or(Rect {
                    x: bounds.x,
                    y: bounds.y + height * index as f64,
                    width: bounds.width,
                    height,
                }),
                confidence: None,
            });
        }
    }
    Ok(output)
}

#[async_trait]
impl OcrBackend for YoudaoOcr {
    async fn recognize(
        &self,
        image: Arc<RgbaImage>,
        _languages: Vec<String>,
        stop: CancellationToken,
    ) -> Result<Vec<OcrLine>> {
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|_| AppError::new(ErrorCode::InvalidInput, "无法编码 OCR 图像"))?;
        let png = png.into_inner();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let salt = format!("0.{}", nonce % 1_000_000_000_000_000_000);
        let boundary = format!("----ScreenTranslate{nonce:x}");
        let mut body = Vec::new();
        let sign = youdao_signature(&png, &salt);
        for (name, value) in [
            ("clientele", "deskdict"),
            ("salt", salt.as_str()),
            ("sign", sign.as_str()),
            ("from", "auto"),
            ("to", "zh-CHS"),
            ("isSaveHistory", "false"),
            ("isSyncSaveHistory", "false"),
            ("funDesc", "photo_translate"),
        ] {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"multipartFile\"; filename=\"capture.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
        body.extend_from_slice(&png);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AppError::new(ErrorCode::Internal, "无法初始化 OCR 请求"))?;
        let request = client.post("https://ocrtran.youdao.com/ocr/imgtranocr").header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/127.0.0.0 Safari/537.36").header("Content-Type", format!("multipart/form-data; boundary={boundary}")).body(body);
        parse_youdao(&json_response(request, &stop).await?)
    }
}

#[async_trait]
impl OcrBackend for AzureOcr {
    async fn recognize(
        &self,
        image: Arc<RgbaImage>,
        _languages: Vec<String>,
        stop: CancellationToken,
    ) -> Result<Vec<OcrLine>> {
        validate_api_url(&self.endpoint)?;
        if self.key.is_empty() {
            return Err(
                AppError::new(ErrorCode::Credentials, "请配置 Azure Vision 密钥").action("ocr"),
            );
        }
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|_| AppError::new(ErrorCode::InvalidInput, "无法编码 OCR 图像"))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AppError::new(ErrorCode::Internal, "无法初始化 OCR 请求"))?;
        let request = client
            .post(format!(
                "{}/computervision/imageanalysis:analyze",
                self.endpoint.trim_end_matches('/')
            ))
            .query(&[("api-version", "2024-02-01"), ("features", "read")])
            .header("Ocp-Apim-Subscription-Key", &self.key)
            .header("Content-Type", "application/octet-stream")
            .body(png.into_inner());
        let value = json_response(request, &stop).await?;
        let blocks = value
            .pointer("/readResult/blocks")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AppError::new(ErrorCode::InvalidResponse, "Azure OCR 响应缺少文字块"))?;
        let mut lines = Vec::new();
        for block in blocks {
            for line in block
                .get("lines")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let Some(text) = line.get("text").and_then(|v| v.as_str()) else {
                    continue;
                };
                let points: Vec<_> = line
                    .get("boundingPolygon")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|p| Some((p["x"].as_f64()?, p["y"].as_f64()?)))
                    .collect();
                if points.is_empty() {
                    continue;
                }
                let x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
                let y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
                let right = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
                let bottom = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
                lines.push(OcrLine {
                    text: text.into(),
                    bounds: Rect {
                        x,
                        y,
                        width: right - x,
                        height: bottom - y,
                    },
                    confidence: None,
                });
            }
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod youdao_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_line_geometry_and_handles_context_only_results() {
        let lines = parse_youdao(&json!({"errorCode":"0","resRegions":[{"boundingBox":"10,20,200,40","lines":[{"text":"Hello", "boundingBox":"11,21,50,12"},{"text":"World"}]},{"boundingBox":"5,80,100,30","context":"第一行\n第二行"}]})).unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].bounds.width, 50.0);
        assert_eq!(lines[1].bounds.y, 40.0);
        assert_eq!(lines[3].bounds.y, 95.0);
        assert!(parse_youdao(&json!({"errorCode":"108"})).is_err());
        assert!(
            parse_youdao(
                &json!({"errorCode":"0","resRegions":[{"boundingBox":"NaN,2,3,4","context":"x"}]})
            )
            .is_err()
        );
    }
}
