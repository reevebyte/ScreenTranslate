mod ai;
mod legacy;
mod official;
mod web;

use async_trait::async_trait;
use reqwest::{Client, Response};
use std::{collections::HashSet, sync::OnceLock, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, ErrorCode, Result, TranslatedItem, TranslationRequest,
    config::{ProviderConfig, ProviderId, validate_api_url},
    ports::TranslatorBackend,
};

fn build_client(allow_https_redirects: bool) -> Result<Client> {
    let policy = if allow_https_redirects {
        reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.stop()
            } else if attempt.url().scheme() == "https" {
                attempt.follow()
            } else {
                attempt.stop()
            }
        })
    } else {
        reqwest::redirect::Policy::none()
    };
    Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(12))
        .redirect(policy)
        .user_agent("ScreenTranslate/2.0")
        .build()
        .map_err(|_| AppError::new(ErrorCode::Internal, "无法初始化网络客户端"))
}

fn shared_client(allow_https_redirects: bool) -> Result<Client> {
    if allow_https_redirects {
        static REDIRECT_CLIENT: OnceLock<Client> = OnceLock::new();
        if let Some(client) = REDIRECT_CLIENT.get() {
            return Ok(client.clone());
        }
        let client = build_client(true)?;
        Ok(REDIRECT_CLIENT.get_or_init(|| client).clone())
    } else {
        static STRICT_CLIENT: OnceLock<Client> = OnceLock::new();
        if let Some(client) = STRICT_CLIENT.get() {
            return Ok(client.clone());
        }
        let client = build_client(false)?;
        Ok(STRICT_CLIENT.get_or_init(|| client).clone())
    }
}

pub(crate) fn apply_opencode_session(
    request: reqwest::RequestBuilder,
    base: &str,
) -> reqwest::RequestBuilder {
    if base.to_ascii_lowercase().contains("opencode") {
        let session = format!(
            "screentranslate-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        request.header("x-opencode-session", session)
    } else {
        request
    }
}

#[derive(Clone)]
pub struct HttpTranslator {
    client: Client,
    pub provider: ProviderId,
    pub options: ProviderConfig,
    key: String,
}

impl HttpTranslator {
    pub fn new(provider: ProviderId, mut options: ProviderConfig, key: String) -> Result<Self> {
        if provider.needs_key() && key.trim().is_empty() {
            return Err(
                AppError::new(ErrorCode::Credentials, "请先为当前翻译服务填写 API 密钥")
                    .action("providers"),
            );
        }
        if options.base_url.trim().is_empty() {
            options.base_url = crate::config::provider_defaults(provider).base_url;
        }
        if !options.base_url.trim().is_empty() {
            validate_api_url(&options.base_url)?;
        }
        // Only BingFree's keyless web page may geo-redirect (e.g. cn.bing.com -> www.bing.com).
        let client = shared_client(provider == ProviderId::BingFree)?;
        Ok(Self {
            client,
            provider,
            options,
            key,
        })
    }

    pub async fn list_models(&self, stop: CancellationToken) -> Result<Vec<String>> {
        if !matches!(self.provider, ProviderId::Openai | ProviderId::Nvidia) {
            return Err(AppError::new(
                ErrorCode::Unsupported,
                "当前服务不提供兼容的模型列表",
            ));
        }
        let base = self.options.base_url.trim_end_matches('/');
        let base = base.strip_suffix("/chat/completions").unwrap_or(base);
        let base = base.strip_suffix("/models").unwrap_or(base);
        let mut request = self.client.get(format!("{base}/models"));
        if !self.key.is_empty() {
            request = request.bearer_auth(&self.key);
        }
        let request = apply_opencode_session(request, base);
        let value = json_response(request, &stop).await?;
        Ok(value
            .get("data")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("id").and_then(|v| v.as_str()).map(str::to_owned))
            .collect())
    }
}

#[async_trait]
impl TranslatorBackend for HttpTranslator {
    async fn translate(
        &self,
        request: TranslationRequest,
        stop: CancellationToken,
    ) -> Result<Vec<TranslatedItem>> {
        if stop.is_cancelled() {
            return Err(AppError::cancelled());
        }
        if matches!(
            self.provider,
            ProviderId::Openai | ProviderId::Nvidia | ProviderId::Anthropic
        ) && self.options.model.trim().is_empty()
        {
            return Err(
                AppError::new(ErrorCode::Configuration, "请先填写模型名称").action("providers")
            );
        }
        if request.items.is_empty() {
            return Ok(Vec::new());
        }
        if request.items.len() > 300
            || request.items.iter().map(|i| i.text.len()).sum::<usize>() > 200_000
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "文字过长，请分段翻译",
            ));
        }
        let ids: HashSet<_> = request.items.iter().map(|item| item.id.as_str()).collect();
        if ids.len() != request.items.len() {
            return Err(AppError::new(ErrorCode::InvalidInput, "文本块 ID 重复"));
        }
        let mut results = Vec::with_capacity(request.items.len());
        for chunk in request.items.chunks(20) {
            let batch = TranslationRequest {
                source: request.source.clone(),
                target: request.target.clone(),
                items: chunk.to_vec(),
            };
            let output = match self.provider {
                ProviderId::GoogleFree => web::google(self, &batch, &stop).await?,
                ProviderId::BingFree
                | ProviderId::MicrosoftFree
                | ProviderId::TencentFree
                | ProviderId::YandexFree
                | ProviderId::IcibaFree => legacy::translate(self, &batch, &stop).await?,
                ProviderId::Openai | ProviderId::Nvidia | ProviderId::Anthropic => {
                    ai::translate(self, &batch, &stop).await?
                }
                _ => official::translate(self, &batch, &stop).await?,
            };
            results.extend(validate_batch(&batch, output)?);
        }
        Ok(results)
    }
}

pub fn validate_batch(
    request: &TranslationRequest,
    output: Vec<TranslatedItem>,
) -> Result<Vec<TranslatedItem>> {
    let invalid = || {
        AppError::new(
            ErrorCode::InvalidResponse,
            "服务返回的文本块不完整，已保留原文，请重试",
        )
    };
    if output.len() != request.items.len() {
        return Err(invalid());
    }
    let mut output: std::collections::HashMap<_, _> =
        output.into_iter().map(|v| (v.id.clone(), v)).collect();
    if output.len() != request.items.len() {
        return Err(invalid());
    }
    let mut results = request
        .items
        .iter()
        .map(|item| output.remove(&item.id).ok_or_else(invalid))
        .collect::<Result<Vec<_>>>()?;
    // A wholly empty reply is broken, but a model may legitimately return
    // nothing for a block such as "—" or an emoji. Keep that block's source
    // text instead of failing the other translations in the batch.
    if results.iter().all(|item| item.text.trim().is_empty()) {
        return Err(invalid());
    }
    for (result, item) in results.iter_mut().zip(&request.items) {
        if result.text.trim().is_empty() {
            result.text = item.text.clone();
        }
    }
    Ok(results)
}

pub async fn json_response(
    request: reqwest::RequestBuilder,
    stop: &CancellationToken,
) -> Result<serde_json::Value> {
    let response = tokio::select! {
        _ = stop.cancelled() => return Err(AppError::cancelled()),
        response = request.send() => response.map_err(|_| AppError::new(ErrorCode::Network, "网络连接失败或请求超时，请检查网络与服务地址"))?,
    };
    let bytes = response_bytes(response, stop).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| AppError::new(ErrorCode::InvalidResponse, "服务返回的内容不是有效 JSON"))
}

fn redact_error_detail(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(240));
    let mut redact_next = false;
    for (idx, token) in raw.split_whitespace().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        let lower = token.to_ascii_lowercase();
        let trimmed = token
            .trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | ')' | '(' | '.' | ':'));
        let lower_trimmed = trimmed.to_ascii_lowercase();
        if redact_next {
            out.push_str("[REDACTED]");
            redact_next = false;
        } else if lower == "bearer" {
            out.push_str(token);
            redact_next = true;
        } else if lower_trimmed.starts_with("sk-")
            || lower_trimmed.starts_with("aiza")
            || lower_trimmed.starts_with("nvapi-")
            || lower_trimmed.starts_with("bearer:")
            || lower_trimmed.contains("api_key=")
            || lower_trimmed.contains("apikey=")
            || lower_trimmed.contains("token=")
        {
            out.push_str("[REDACTED]");
        } else {
            out.push_str(token);
        }
        if out.chars().count() >= 240 {
            break;
        }
    }
    out.chars().take(240).collect()
}

pub(crate) async fn response_bytes(
    response: Response,
    stop: &CancellationToken,
) -> Result<Vec<u8>> {
    const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
    const ERROR_DETAIL_LIMIT: usize = 64 * 1024;

    let status = response.status();
    if !status.is_success() {
        let code = match status.as_u16() {
            401 | 403 => ErrorCode::Authentication,
            429 => ErrorCode::RateLimited,
            _ => ErrorCode::Network,
        };
        let fallback_msg = match code {
            ErrorCode::Authentication => "服务拒绝了请求，请检查密钥、区域或账户权限",
            ErrorCode::RateLimited => "服务请求过于频繁，请稍后重试",
            _ => "服务暂时无法处理请求，请检查配置或稍后重试",
        };
        let err_detail = if code == ErrorCode::Network {
            read_limited_response(response, stop, ERROR_DETAIL_LIMIT)
                .await
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| {
                    value
                        .pointer("/error/message")
                        .and_then(|v| v.as_str())
                        .or_else(|| value.get("message").and_then(|v| v.as_str()))
                        .map(redact_error_detail)
                })
        } else {
            None
        };
        let msg = if let Some(detail) = err_detail.filter(|d| !d.is_empty()) {
            format!("{fallback_msg} ({detail})")
        } else {
            fallback_msg.to_string()
        };
        return Err(AppError::new(code, msg).action("providers"));
    }
    read_limited_response(response, stop, RESPONSE_LIMIT).await
}

async fn read_limited_response(
    mut response: Response,
    stop: &CancellationToken,
    limit: usize,
) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err(AppError::new(ErrorCode::InvalidResponse, "服务响应过大"));
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = stop.cancelled() => return Err(AppError::cancelled()),
            chunk = response.chunk() => chunk.map_err(|_| AppError::new(ErrorCode::Network, "服务连接在接收数据时中断"))?,
        };
        let Some(chunk) = chunk else { break };
        if bytes.len() + chunk.len() > limit {
            return Err(AppError::new(ErrorCode::InvalidResponse, "服务响应过大"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TranslationItem, config::provider_defaults};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn request() -> TranslationRequest {
        TranslationRequest {
            source: "auto".into(),
            target: "zh-Hans".into(),
            items: vec![TranslationItem {
                id: "a".into(),
                text: "hello".into(),
            }],
        }
    }

    #[tokio::test]
    async fn discovers_models_before_selection_but_requires_one_for_translation() {
        for provider in [ProviderId::Openai, ProviderId::Nvidia] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/models"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"data":[{"id":"fixture-model"}]})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let backend = HttpTranslator::new(
                provider,
                ProviderConfig {
                    base_url: server.uri(),
                    ..provider_defaults(provider)
                },
                "fixture".into(),
            )
            .unwrap();
            assert_eq!(
                backend.list_models(CancellationToken::new()).await.unwrap(),
                vec!["fixture-model"]
            );
            assert_eq!(
                backend
                    .translate(request(), CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Configuration
            );
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn microsoft_parser_and_authentication_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/translate"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"translations":[{"text":"你好"}]}])),
            )
            .mount(&server)
            .await;
        let backend = HttpTranslator::new(
            ProviderId::Microsoft,
            ProviderConfig {
                base_url: server.uri(),
                ..provider_defaults(ProviderId::Microsoft)
            },
            "test-fixture".into(),
        )
        .unwrap();
        assert_eq!(
            backend
                .translate(request(), CancellationToken::new())
                .await
                .unwrap()[0]
                .text,
            "你好"
        );
        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(401).set_body_string("secret must never be exposed"),
            )
            .mount(&server)
            .await;
        let error = backend
            .translate(request(), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Authentication);
        assert!(!error.message.contains("secret"));
    }

    #[tokio::test]
    async fn cancellation_interrupts_pending_network_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let backend = HttpTranslator::new(
            ProviderId::Microsoft,
            ProviderConfig {
                base_url: server.uri(),
                ..provider_defaults(ProviderId::Microsoft)
            },
            "fixture".into(),
        )
        .unwrap();
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        });
        let error =
            tokio::time::timeout(Duration::from_secs(1), backend.translate(request(), token))
                .await
                .unwrap()
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn rate_limits_malformed_and_incomplete_responses_are_recoverable() {
        let server = MockServer::start().await;
        let backend = HttpTranslator::new(
            ProviderId::Microsoft,
            ProviderConfig {
                base_url: server.uri(),
                ..provider_defaults(ProviderId::Microsoft)
            },
            "fixture".into(),
        )
        .unwrap();
        for (response, expected) in [
            (
                ResponseTemplate::new(429).set_body_string("private response"),
                ErrorCode::RateLimited,
            ),
            (
                ResponseTemplate::new(200).set_body_string("private invalid JSON"),
                ErrorCode::InvalidResponse,
            ),
            (
                ResponseTemplate::new(200).set_body_json(serde_json::json!([])),
                ErrorCode::InvalidResponse,
            ),
        ] {
            server.reset().await;
            Mock::given(method("POST"))
                .respond_with(response)
                .mount(&server)
                .await;
            let error = backend
                .translate(request(), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, expected);
            assert!(!error.message.contains("private"));
        }
    }

    #[tokio::test]
    async fn request_timeout_is_redacted_and_does_not_return_a_partial_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
            .mount(&server)
            .await;
        let client = Client::builder()
            .timeout(Duration::from_millis(25))
            .build()
            .unwrap();
        let error = json_response(
            client.get(format!("{}/private-path", server.uri())),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Network);
        assert!(!error.message.contains("private-path"));
    }

    #[tokio::test]
    async fn oversized_error_details_are_ignored() {
        let server = MockServer::start().await;
        let oversized_detail = format!(
            r#"{{"message":"{}"}}"#,
            "untrusted-error-detail".repeat(4 * 1024)
        );
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_string(oversized_detail))
            .mount(&server)
            .await;
        let client = Client::builder().build().unwrap();
        let response = client.get(server.uri()).send().await.unwrap();
        let error = response_bytes(response, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Network);
        assert!(!error.message.contains("untrusted-error-detail"));
    }

    #[test]
    fn mismatched_ids_are_rejected_and_order_is_restored() {
        assert!(
            validate_batch(
                &request(),
                vec![TranslatedItem {
                    id: "wrong".into(),
                    text: "你好".into()
                }]
            )
            .is_err()
        );
        assert!(validate_batch(&request(), Vec::new()).is_err());
    }

    #[test]
    fn one_empty_translation_keeps_its_source_but_an_all_empty_reply_fails() {
        let mut batch = request();
        batch.items.push(TranslationItem {
            id: "b".into(),
            text: "—".into(),
        });
        let item = |id: &str, text: &str| TranslatedItem {
            id: id.into(),
            text: text.into(),
        };
        let results = validate_batch(&batch, vec![item("b", " "), item("a", "你好")]).unwrap();
        assert_eq!(results[0].text, "你好");
        assert_eq!(results[1].text, "—");
        assert!(validate_batch(&batch, vec![item("a", ""), item("b", "")]).is_err());
    }

    #[test]
    fn redacts_tokens_and_keys_in_remote_error_details() {
        let scrubbed = redact_error_detail(
            "Invalid key sk-secret123456 with Bearer eyJhbGciOi and api_key=abc999",
        );
        assert!(!scrubbed.contains("sk-secret123456"));
        assert!(!scrubbed.contains("eyJhbGciOi"));
        assert!(!scrubbed.contains("abc999"));
        assert!(scrubbed.contains("[REDACTED]"));
    }
}
