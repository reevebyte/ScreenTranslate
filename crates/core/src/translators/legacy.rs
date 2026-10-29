//! Adapters carried forward from the original native client.
use super::{HttpTranslator, json_response, response_bytes};
use crate::{AppError, ErrorCode, Result, TranslatedItem, TranslationRequest, config::ProviderId};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

const AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/127.0.0.0 Safari/537.36";
fn invalid() -> AppError {
    AppError::new(
        ErrorCode::InvalidResponse,
        "免密接口响应异常，请重试或切换其他服务",
    )
    .action("providers")
}
fn nonce() -> String {
    format!(
        "{:032x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
fn code(provider: ProviderId, value: &str) -> &str {
    match (provider, value) {
        (ProviderId::TencentFree | ProviderId::IcibaFree | ProviderId::YandexFree, "zh-Hans") => {
            "zh"
        }
        (ProviderId::TencentFree, "zh-Hant") => "zh-TW",
        (ProviderId::IcibaFree, "zh-Hant") => "cht",
        (ProviderId::YandexFree, "zh-Hant") => "zh",
        _ => value,
    }
}
fn base(backend: &HttpTranslator) -> &str {
    if !backend.options.base_url.is_empty() {
        return backend.options.base_url.trim_end_matches('/');
    }
    match backend.provider {
        ProviderId::BingFree => "https://cn.bing.com",
        ProviderId::MicrosoftFree => "https://api.cognitive.microsofttranslator.com",
        ProviderId::TencentFree => "https://transmart.qq.com",
        ProviderId::YandexFree => "https://translate.yandex.net",
        ProviderId::IcibaFree => "https://dictionary.iciba.com",
        _ => unreachable!(),
    }
}
fn microsoft_signature(url: &reqwest::Url) -> String {
    const KEY: [u8; 64] = [
        0xA2, 0x29, 0x3A, 0x3D, 0xD0, 0xDD, 0x32, 0x73, 0x97, 0x7A, 0x64, 0xDB, 0xC2, 0xF3, 0x27,
        0xF5, 0xD7, 0xBF, 0x87, 0xD9, 0x45, 0x9D, 0xF0, 0x5A, 0x09, 0x66, 0xC6, 0x30, 0xC6, 0x6A,
        0xAA, 0x84, 0x9A, 0x41, 0xAA, 0x94, 0x3A, 0xA8, 0xD5, 0x1A, 0x6E, 0x4D, 0xAA, 0xC9, 0xA3,
        0x70, 0x12, 0x35, 0xC7, 0xEB, 0x12, 0xF6, 0xE8, 0x23, 0x07, 0x9E, 0x47, 0x10, 0x95, 0x91,
        0x88, 0x55, 0xD8, 0x17,
    ];
    let path = url.as_str().split_once("://").unwrap().1;
    let encoded: String = url::form_urlencoded::byte_serialize(path.as_bytes()).collect();
    let date = httpdate::fmt_http_date(SystemTime::now()).replace(" GMT", "GMT");
    let id = nonce();
    let raw = format!("MSTranslatorAndroidApp{encoded}{date}{id}").to_lowercase();
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(&KEY).expect("fixed HMAC key");
    mac.update(raw.as_bytes());
    let digest = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    format!("MSTranslatorAndroidApp::{digest}::{date}::{id}")
}
struct BingToken {
    key: String,
    token: String,
    ig: String,
    iid: String,
}
fn bing_token(html: &str) -> Result<BingToken> {
    fn capture(html: &str, pattern: &str, group: usize) -> Result<String> {
        regex::Regex::new(pattern)
            .unwrap()
            .captures(html)
            .and_then(|v| v.get(group))
            .map(|v| v.as_str().to_owned())
            .ok_or_else(invalid)
    }
    let pattern = r#"params_AbusePreventionHelper\s*=\s*\[\s*([^,\]]+)\s*,\s*"([^"]+)""#;
    Ok(BingToken {
        key: capture(html, pattern, 1)?
            .trim()
            .trim_matches('"')
            .to_owned(),
        token: capture(html, pattern, 2)?,
        ig: capture(html, r#"IG:\s*"([^"]+)""#, 1)?,
        iid: capture(html, r#"data-iid="([^"]+)""#, 1).unwrap_or_else(|_| "translator.5023".into()),
    })
}
fn parse_one(provider: ProviderId, value: &Value) -> Result<String> {
    let text = match provider {
        ProviderId::BingFree | ProviderId::MicrosoftFree => value
            .pointer("/0/translations/0/text")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ProviderId::TencentFree => value["auto_translation"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                value["auto_translation"].as_array().map(|parts| {
                    parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }),
        ProviderId::YandexFree => value
            .pointer("/text/0")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    };
    text.filter(|v| !v.trim().is_empty()).ok_or_else(invalid)
}
pub async fn translate(
    backend: &HttpTranslator,
    input: &TranslationRequest,
    stop: &CancellationToken,
) -> Result<Vec<TranslatedItem>> {
    let base = base(backend);
    let source = code(backend.provider, &input.source);
    let target = code(backend.provider, &input.target);
    if backend.provider == ProviderId::IcibaFree {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string();
        let signature = format!(
            "{:x}",
            md5::compute(format!(
                "/dictionary/fy/batch61000006{timestamp}7ece94d9f9c202b0d2ec557dg4r9bc"
            ))
        );
        let value = json_response(backend.client.post(format!("{base}/dictionary/fy/batch")).query(&[("client", "6"), ("key", "1000006"), ("timestamp", timestamp.as_str()), ("signature", signature.as_str())]).header("Origin", "https://www.iciba.com").header("Referer", "https://www.iciba.com/").header("User-Agent", AGENT).json(&json!({"from":source,"to":target,"textList":input.items.iter().map(|v| &v.text).collect::<Vec<_>>()})), stop).await?;
        if value["code"] != 1 && value["code"] != "1" {
            return Err(invalid());
        }
        let data = value["data"]
            .as_array()
            .filter(|v| v.len() == input.items.len())
            .ok_or_else(invalid)?;
        return input
            .items
            .iter()
            .zip(data)
            .map(|(item, value)| {
                Ok(TranslatedItem {
                    id: item.id.clone(),
                    text: value
                        .as_str()
                        .or_else(|| value["out"].as_str())
                        .filter(|v| !v.trim().is_empty())
                        .ok_or_else(invalid)?
                        .to_owned(),
                })
            })
            .collect();
    }
    let token = if backend.provider == ProviderId::BingFree {
        let request = backend
            .client
            .get(format!("{base}/translator"))
            .header("User-Agent", AGENT);
        let response = tokio::select! { _ = stop.cancelled() => return Err(AppError::cancelled()), value = request.send() => value.map_err(|_| AppError::new(ErrorCode::Network, "无法连接必应翻译"))? };
        Some(bing_token(
            &String::from_utf8(response_bytes(response, stop).await?).map_err(|_| invalid())?,
        )?)
    } else {
        None
    };
    let mut output = Vec::with_capacity(input.items.len());
    for item in &input.items {
        let request = match backend.provider {
            ProviderId::BingFree => {
                let token = token.as_ref().unwrap();
                backend.client.post(format!("{base}/ttranslatev3")).query(&[("isVertical", "1"), ("IG", token.ig.as_str()), ("IID", token.iid.as_str())]).header("Referer", format!("{base}/translator")).header("Origin", base).form(&[("fromLang", if source == "auto" { "auto-detect" } else { source }), ("to", target), ("text", item.text.as_str()), ("token", &token.token), ("key", &token.key)])
            }
            ProviderId::MicrosoftFree => {
                let mut url = reqwest::Url::parse(&format!("{base}/translate")).map_err(|_| invalid())?;
                url.query_pairs_mut().append_pair("api-version", "3.0").append_pair("to", target);
                if source != "auto" { url.query_pairs_mut().append_pair("from", source); }
                let signature = microsoft_signature(&url);
                backend.client.post(url).header("X-MT-Signature", signature).json(&json!([{"Text":item.text}]))
            }
            ProviderId::TencentFree => backend.client.post(format!("{base}/api/imt")).header("Referer", "https://yi.qq.com/zh-CN/index").json(&json!({"header":{"fn":"auto_translation_block","client_key":format!("browser-chrome-110.0.0-Mac OS-{}",nonce())},"type":"plain","model_category":"normal","source":{"lang":source,"text_block":item.text},"target":{"lang":target}})),
            ProviderId::YandexFree => {
                let language = if source == "auto" { target.to_owned() } else { format!("{source}-{target}") };
                backend.client.post(format!("{base}/api/v1/tr.json/translate")).query(&[("ucid", nonce().as_str()), ("srv", "android"), ("format", "text")]).form(&[("text", item.text.as_str()), ("lang", language.as_str())])
            }
            _ => unreachable!(),
        }.header("User-Agent", if backend.provider == ProviderId::YandexFree { "ru.yandex.translate/3.20.2024" } else { AGENT });
        output.push(TranslatedItem {
            id: item.id.clone(),
            text: parse_one(backend.provider, &json_response(request, stop).await?)?,
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TranslationItem, config::ProviderConfig, ports::TranslatorBackend};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header_exists, method, path},
    };

    #[tokio::test]
    #[ignore = "sends only Hello to the five public translation interfaces"]
    async fn online_public_interfaces() {
        let mut checks = tokio::task::JoinSet::new();
        for provider in [
            ProviderId::BingFree,
            ProviderId::MicrosoftFree,
            ProviderId::TencentFree,
            ProviderId::YandexFree,
            ProviderId::IcibaFree,
        ] {
            checks.spawn(async move {
                let backend = HttpTranslator::new(
                    provider,
                    crate::config::provider_defaults(provider),
                    String::new(),
                )
                .unwrap();
                let request = TranslationRequest {
                    source: "en".into(),
                    target: "zh-Hans".into(),
                    items: vec![TranslationItem {
                        id: "test".into(),
                        text: "Hello".into(),
                    }],
                };
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    backend.translate(request, CancellationToken::new()),
                )
                .await;
                let success = matches!(&result, Ok(Ok(_)));
                eprintln!(
                    "{}: {}",
                    provider.key(),
                    match result {
                        Ok(Ok(_)) => "ok".into(),
                        Ok(Err(error)) => error.message,
                        Err(_) => "timeout".into(),
                    }
                );
                success
            });
        }
        let mut passed = 0;
        while let Some(result) = checks.join_next().await {
            if result.unwrap() {
                passed += 1;
            }
        }
        assert_eq!(passed, 5);
    }
    #[tokio::test]
    async fn all_restored_adapters_make_real_http_requests_and_keep_item_ids() {
        for (provider, route, response) in [
            (
                ProviderId::BingFree,
                "/ttranslatev3",
                json!([{"translations":[{"text":"你好"}]}]),
            ),
            (
                ProviderId::MicrosoftFree,
                "/translate",
                json!([{"translations":[{"text":"你好"}]}]),
            ),
            (
                ProviderId::TencentFree,
                "/api/imt",
                json!({"auto_translation":["你好"]}),
            ),
            (
                ProviderId::YandexFree,
                "/api/v1/tr.json/translate",
                json!({"text":["你好"]}),
            ),
            (
                ProviderId::IcibaFree,
                "/dictionary/fy/batch",
                json!({"code":1,"data":[{"out":"你好"}]}),
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET")).and(path("/translator")).respond_with(ResponseTemplate::new(200).set_body_string(r#"IG:"ig-test"; params_AbusePreventionHelper = [123,"token-test",10]; <div data-iid="translator.1">"#)).mount(&server).await;
            let mut mock = Mock::given(method("POST")).and(path(route));
            if provider == ProviderId::MicrosoftFree {
                mock = mock.and(header_exists("X-MT-Signature"));
            }
            mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
                .expect(1)
                .mount(&server)
                .await;
            let backend = HttpTranslator::new(
                provider,
                ProviderConfig {
                    base_url: server.uri(),
                    ..Default::default()
                },
                String::new(),
            )
            .unwrap();
            let result = backend
                .translate(
                    TranslationRequest {
                        source: "auto".into(),
                        target: "zh-Hans".into(),
                        items: vec![TranslationItem {
                            id: "same-id".into(),
                            text: "Hello".into(),
                        }],
                    },
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(result[0].id, "same-id");
            assert_eq!(result[0].text, "你好");
        }
    }
}
