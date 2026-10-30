mod ai;
mod legacy;
mod official;
mod web;

use async_trait::async_trait;
use reqwest::{Client, Response};
use std::{collections::HashSet, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, ErrorCode, Result, TranslatedItem, TranslationRequest,
    config::{ProviderConfig, ProviderId, validate_api_url},
    ports::TranslatorBackend,
};

#[derive(Clone)]
pub struct HttpTranslator {
    client: Client,
    pub provider: ProviderId,
    pub options: ProviderConfig,
    key: String,
}

impl HttpTranslator {
    pub fn new(provider: ProviderId, options: ProviderConfig, key: String) -> Result<Self> {
        if provider.needs_key() && key.trim().is_empty() {
            return Err(
                AppError::new(ErrorCode::Credentials, "请先为当前翻译服务填写 API 密钥")
                    .action("providers"),
            );
        }
        if !options.base_url.is_empty() {
            validate_api_url(&options.base_url)?;
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("ScreenTranslate/2.0")
            .build()
            .map_err(|_| AppError::new(ErrorCode::Internal, "无法初始化网络客户端"))?;
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
        let mut request = self.client.get(format!(
            "{}/models",
            self.options.base_url.trim_end_matches('/')
        ));
        if !self.key.is_empty() {
            request = request.bearer_auth(&self.key);
        }
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
    request
        .items
        .iter()
        .map(|item| {
            output
                .remove(&item.id)
                .filter(|v| !v.text.trim().is_empty())
                .ok_or_else(invalid)
        })
        .collect()
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

pub(crate) async fn response_bytes(
    mut response: Response,
    stop: &CancellationToken,
) -> Result<Vec<u8>> {
    let code = match response.status().as_u16() {
        401 | 403 => Some((
            ErrorCode::Authentication,
            "服务拒绝了请求，请检查密钥、区域或账户权限",
        )),
        429 => Some((ErrorCode::RateLimited, "服务请求过于频繁，请稍后重试")),
        200..=299 => None,
        _ => Some((
            ErrorCode::Network,
            "服务暂时无法处理请求，请检查配置或稍后重试",
        )),
    };
    if let Some((code, message)) = code {
        return Err(AppError::new(code, message).action("providers"));
    }
    const LIMIT: usize = 4 * 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|len| len > LIMIT as u64)
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
        if bytes.len() + chunk.len() > LIMIT {
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
}
