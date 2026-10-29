use async_trait::async_trait;
use base64::Engine;
use image::RgbaImage;
use screentrans_core::{
    AppError, ErrorCode, OcrLine, Rect, Result,
    config::{CustomOcrConfig, OcrProtocol, validate_api_url},
    ports::OcrBackend,
    translators::json_response,
};
use serde_json::{Value, json};
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

pub struct CustomOcr {
    pub options: CustomOcrConfig,
    pub key: String,
}

fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidResponse,
        "OCR 响应格式或文字坐标无效，请检查自定义接口协议",
    )
    .action("ocr")
}

pub fn parse_response(value: &Value, width: u32, height: u32) -> Result<Vec<OcrLine>> {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f64,
        height: height as f64,
    };
    if let Some(blocks) = value
        .get("blocks")
        .or_else(|| value.get("lines"))
        .and_then(Value::as_array)
    {
        if blocks.len() > 4096 {
            return Err(invalid());
        }
        let mut lines = Vec::new();
        let mut total_text = 0;
        for block in blocks {
            let text = block
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?
                .trim();
            if text.is_empty() {
                continue;
            }
            total_text += text.len();
            if total_text > 100_000 {
                return Err(invalid());
            }
            let b = block.get("bounds").unwrap_or(block);
            let bounds = Rect {
                x: b["x"].as_f64().ok_or_else(invalid)?,
                y: b["y"].as_f64().ok_or_else(invalid)?,
                width: b["width"].as_f64().ok_or_else(invalid)?,
                height: b["height"].as_f64().ok_or_else(invalid)?,
            };
            bounds.validate().map_err(|_| invalid())?;
            if bounds.x < 0.0
                || bounds.y < 0.0
                || bounds.right() > full.width + 1.0
                || bounds.bottom() > full.height + 1.0
            {
                return Err(invalid());
            }
            let confidence = block["confidence"]
                .as_f64()
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                .map(|v| v as f32);
            lines.push(OcrLine {
                text: text.into(),
                bounds,
                confidence,
            });
        }
        return Ok(lines);
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        if text.len() > 100_000 {
            return Err(invalid());
        }
        return Ok(if text.trim().is_empty() {
            vec![]
        } else {
            vec![OcrLine {
                text: text.trim().into(),
                bounds: full,
                confidence: None,
            }]
        });
    }
    Err(invalid())
}

#[async_trait]
impl OcrBackend for CustomOcr {
    async fn recognize(
        &self,
        image: Arc<RgbaImage>,
        languages: Vec<String>,
        stop: CancellationToken,
    ) -> Result<Vec<OcrLine>> {
        let mut endpoint = validate_api_url(&self.options.endpoint).map_err(|_| {
            AppError::new(ErrorCode::Configuration, "请填写有效的自定义 OCR 地址").action("ocr")
        })?;
        if self.options.protocol == OcrProtocol::Openai && self.options.model.trim().is_empty() {
            return Err(
                AppError::new(ErrorCode::Configuration, "请填写视觉模型名称").action("ocr"),
            );
        }
        let (width, height) = image.dimensions();
        let encode = tokio::task::spawn_blocking(move || {
            let mut out = Cursor::new(Vec::new());
            image
                .write_to(&mut out, image::ImageFormat::Png)
                .map_err(|_| invalid())?;
            Ok::<_, AppError>(out.into_inner())
        });
        let png = tokio::select! {_ = stop.cancelled()=>return Err(AppError::cancelled()),r=encode=>r.map_err(|_|AppError::new(ErrorCode::Internal,"OCR 图像编码失败"))??};
        if png.len() > 32 * 1024 * 1024 {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "OCR 图像过大，请缩小选区",
            ));
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let body = match self.options.protocol {
            OcrProtocol::HttpJson => {
                json!({"image":encoded,"format":"png","width":width,"height":height,"languages":languages})
            }
            OcrProtocol::Openai => {
                if !endpoint
                    .path()
                    .trim_end_matches('/')
                    .ends_with("/chat/completions")
                {
                    endpoint.set_path(&format!(
                        "{}/chat/completions",
                        endpoint.path().trim_end_matches('/')
                    ));
                }
                let prompt = format!(
                    "Recognize every visible text line in this {width} by {height} pixel image. Return only JSON: {{\"blocks\":[{{\"text\":\"...\",\"bounds\":{{\"x\":0,\"y\":0,\"width\":100,\"height\":20}}}}]}}. Coordinates must be physical pixels within this image. Preserve original language and line content. No translation. Languages: {}",
                    languages.join(",")
                );
                json!({"model":self.options.model,"messages":[{"role":"user","content":[{"type":"text","text":prompt},{"type":"image_url","image_url":{"url":format!("data:image/png;base64,{encoded}")}}]}]})
            }
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AppError::new(ErrorCode::Internal, "无法初始化 OCR 请求"))?;
        let mut request = client.post(endpoint).json(&body);
        if !self.key.is_empty() {
            request = request.bearer_auth(&self.key);
        }
        let value = json_response(request, &stop).await.map_err(|mut error| {
            if error.code != ErrorCode::Cancelled {
                error.action = Some("ocr".into());
            }
            error
        })?;
        let parsed = if self.options.protocol == OcrProtocol::Openai {
            let content = value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?
                .trim();
            let content = content
                .strip_prefix("```json")
                .or_else(|| content.strip_prefix("```"))
                .unwrap_or(content)
                .trim();
            let content = content.strip_suffix("```").unwrap_or(content).trim();
            serde_json::from_str(content).map_err(|_| invalid())?
        } else {
            value
        };
        parse_response(&parsed, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_positioned_blocks_and_text_only_without_inventing_line_positions() {
        let lines = parse_response(
            &json!({"blocks":[{"text":"Hello","bounds":{"x":2,"y":3,"width":20,"height":12}}]}),
            100,
            60,
        )
        .unwrap();
        assert_eq!(lines[0].bounds.x, 2.0);
        let text = parse_response(&json!({"text":"one\ntwo"}), 100, 60).unwrap();
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].bounds.width, 100.0);
        assert!(
            parse_response(
                &json!({"blocks":[{"text":"bad","bounds":{"x":2,"y":3,"width":200,"height":12}}]}),
                100,
                60
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };
    fn image() -> Arc<RgbaImage> {
        Arc::new(RgbaImage::new(100, 60))
    }
    fn backend(endpoint: String, protocol: OcrProtocol) -> CustomOcr {
        CustomOcr {
            options: CustomOcrConfig {
                endpoint,
                protocol,
                model: "fixture-vision".into(),
            },
            key: String::new(),
        }
    }
    #[tokio::test]
    async fn posts_png_with_dimensions_languages_and_optional_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ocr"))
            .and(header("Authorization", "Bearer fixture-secret"))
            .and(body_partial_json(
                json!({"format":"png","width":100,"height":60,"languages":["en"]}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"blocks":[{"text":"Hello","bounds":{"x":2,"y":3,"width":50,"height":20}}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let mut api = backend(format!("{}/ocr", server.uri()), OcrProtocol::HttpJson);
        api.key = "fixture-secret".into();
        let result = api
            .recognize(image(), vec!["en".into()], CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result[0].text, "Hello");
        let request = &server.received_requests().await.unwrap()[0];
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(body["image"].as_str().unwrap())
            .unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    #[tokio::test]
    async fn vision_accepts_fenced_json_and_full_endpoint_without_duplicating_suffix() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"model":"fixture-vision"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"choices":[{"message":{"content":"```json\n{\"text\":\"Hello\"}\n```"}}]}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        for endpoint in [
            format!("{}/v1", server.uri()),
            format!("{}/v1/chat/completions", server.uri()),
        ] {
            assert_eq!(
                backend(endpoint, OcrProtocol::Openai)
                    .recognize(image(), vec![], CancellationToken::new())
                    .await
                    .unwrap()[0]
                    .text,
                "Hello"
            );
        }
        for request in server.received_requests().await.unwrap() {
            assert!(request.headers.get("authorization").is_none());
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert!(
                body.pointer("/messages/0/content/1/image_url/url")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .starts_with("data:image/png;base64,")
            );
        }
    }
    #[tokio::test]
    async fn reports_auth_rate_limits_and_malformed_responses_at_ocr_settings() {
        let server = MockServer::start().await;
        for (status, expected) in [
            (401, ErrorCode::Authentication),
            (429, ErrorCode::RateLimited),
            (200, ErrorCode::InvalidResponse),
        ] {
            server.reset().await;
            Mock::given(method("POST"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_string("invalid response; fixture-secret"),
                )
                .mount(&server)
                .await;
            let error = backend(server.uri(), OcrProtocol::HttpJson)
                .recognize(image(), vec![], CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, expected);
            assert_eq!(error.action.as_deref(), Some("ocr"));
            assert!(!error.message.contains("fixture-secret"));
        }
    }
    #[tokio::test]
    async fn cancels_pending_network_requests() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(10))
                    .set_body_json(json!({"text":"late"})),
            )
            .mount(&server)
            .await;
        let token = CancellationToken::new();
        let request = backend(server.uri(), OcrProtocol::HttpJson);
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel.cancel();
        });
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            request.recognize(image(), vec![], token),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Cancelled);
    }
    #[test]
    fn rejects_missing_negative_zero_and_outside_coordinates_and_excessive_text() {
        for bounds in [
            json!({}),
            json!({"x":-1,"y":0,"width":1,"height":1}),
            json!({"x":0,"y":0,"width":0,"height":1}),
            json!({"x":99,"y":0,"width":3,"height":1}),
        ] {
            assert!(
                parse_response(&json!({"blocks":[{"text":"x","bounds":bounds}]}), 100, 60).is_err()
            );
        }
        let block = json!({"text":"x".repeat(60000),"bounds":{"x":0,"y":0,"width":1,"height":1}});
        assert!(parse_response(&json!({"blocks":[block.clone(),block]}), 100, 60).is_err());
        assert!(parse_response(&json!({"blocks":vec![json!({"text":""});4097]}), 100, 60).is_err());
    }
}
