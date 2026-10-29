use super::{HttpTranslator, json_response};
use crate::{AppError, ErrorCode, Result, TranslatedItem, TranslationRequest, config::ProviderId};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub async fn translate(
    backend: &HttpTranslator,
    input: &TranslationRequest,
    stop: &CancellationToken,
) -> Result<Vec<TranslatedItem>> {
    let base = backend.options.base_url.trim_end_matches('/');
    let automatic = input.source == "auto" || input.source == "other";
    let texts: Vec<_> = input.items.iter().map(|item| item.text.as_str()).collect();
    let value = match backend.provider {
        ProviderId::Microsoft => {
            let mut request = backend
                .client
                .post(format!("{base}/translate"))
                .header("Ocp-Apim-Subscription-Key", &backend.key)
                .query(&[
                    ("api-version", "3.0"),
                    ("to", input.target.as_str()),
                    ("textType", "plain"),
                ]);
            if !backend.options.region.is_empty() {
                request = request.header("Ocp-Apim-Subscription-Region", &backend.options.region);
            }
            if !automatic {
                request = request.query(&[("from", &input.source)]);
            }
            json_response(
                request.json(
                    &texts
                        .iter()
                        .map(|text| json!({"Text": text}))
                        .collect::<Vec<_>>(),
                ),
                stop,
            )
            .await?
        }
        ProviderId::Deepl => {
            let target = match input.target.as_str() {
                "zh-Hans" => "ZH-HANS".into(),
                "zh-Hant" => "ZH-HANT".into(),
                other => other.to_uppercase(),
            };
            let mut body = json!({"text": texts, "target_lang": target});
            if !automatic {
                body["source_lang"] = input.source.to_uppercase().into();
            }
            json_response(
                backend
                    .client
                    .post(format!("{base}/translate"))
                    .header("Authorization", format!("DeepL-Auth-Key {}", backend.key))
                    .json(&body),
                stop,
            )
            .await?
        }
        ProviderId::Google => {
            let mut body =
                json!({ "q": texts, "target": google_code(&input.target), "format": "text" });
            if !automatic {
                body["source"] = google_code(&input.source).into();
            }
            json_response(
                backend
                    .client
                    .post(base)
                    .header("X-goog-api-key", &backend.key)
                    .json(&body),
                stop,
            )
            .await?
        }
        _ => return Err(AppError::new(ErrorCode::Unsupported, "不支持的翻译接口")),
    };
    let array = match backend.provider {
        ProviderId::Microsoft => value.as_array(),
        ProviderId::Deepl => value.get("translations").and_then(Value::as_array),
        ProviderId::Google => value
            .pointer("/data/translations")
            .and_then(Value::as_array),
        _ => None,
    }
    .ok_or_else(|| AppError::new(ErrorCode::InvalidResponse, "翻译服务返回的数据结构无效"))?;
    if array.len() != input.items.len() {
        return Err(AppError::new(
            ErrorCode::InvalidResponse,
            "翻译服务返回的文本数量不匹配",
        ));
    }
    input
        .items
        .iter()
        .zip(array)
        .map(|(item, translated)| {
            let text = match backend.provider {
                ProviderId::Microsoft => translated.pointer("/translations/0/text"),
                ProviderId::Deepl => translated.get("text"),
                _ => translated.get("translatedText"),
            }
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::new(ErrorCode::InvalidResponse, "翻译结果缺少文字"))?;
            Ok(TranslatedItem {
                id: item.id.clone(),
                text: text.into(),
            })
        })
        .collect()
}

pub(crate) fn google_code(code: &str) -> &str {
    match code {
        "zh-Hans" => "zh-CN",
        "zh-Hant" => "zh-TW",
        other => other,
    }
}
