use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use gateway_core::{
    ChatCompletionsRequest, EmbeddingsRequest, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream,
};
use serde_json::Value;

use crate::http::{join_base_url, map_reqwest_error};

#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    pub provider_key: String,
    pub base_url: String,
    pub bearer_token: Option<String>,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

impl OpenAiCompatConfig {
    #[must_use]
    pub fn new(provider_key: String, base_url: String) -> Self {
        Self {
            provider_key,
            base_url,
            bearer_token: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 120_000,
        }
    }
}

#[derive(Clone)]
pub struct OpenAiCompatProvider {
    config: OpenAiCompatConfig,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn new(config: OpenAiCompatConfig) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self { config, client })
    }

    pub fn build_chat_request(
        &self,
        request: &ChatCompletionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let mut body = serde_json::to_value(request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        self.build_request("chat/completions", body, context)
    }

    pub fn build_embeddings_request(
        &self,
        request: &EmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let mut body = serde_json::to_value(request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        self.build_request("embeddings", body, context)
    }

    fn build_request(
        &self,
        endpoint_suffix: &str,
        mut body: Value,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        if let Some(object) = body.as_object_mut() {
            for (key, value) in &context.extra_body {
                object.insert(key.clone(), value.clone());
            }
        }

        let url = join_base_url(&self.config.base_url, endpoint_suffix)?;

        let mut request = self.client.post(url).json(&body);

        for (header_name, header_value) in &self.config.default_headers {
            request = request.header(header_name, header_value);
        }
        for (header_name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                request = request.header(header_name, value);
            }
        }

        request = request.header("x-request-id", &context.request_id);
        if let Some(idempotency_key) = &context.idempotency_key {
            request = request.header("Idempotency-Key", idempotency_key);
        }

        if let Some(bearer_token) = &self.config.bearer_token {
            request = request.bearer_auth(bearer_token);
        }

        request.build().map_err(map_reqwest_error)
    }

    async fn execute_json_request(
        &self,
        request: reqwest::Request,
    ) -> Result<Value, ProviderError> {
        let response = self
            .client
            .execute(request)
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        let text = response.text().await.map_err(map_reqwest_error)?;

        if !status.is_success() {
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: text,
            });
        }

        serde_json::from_str(&text).map_err(|error| ProviderError::Transport(error.to_string()))
    }
}

#[async_trait]
impl ProviderClient for OpenAiCompatProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::openai_compat_baseline()
    }

    async fn chat_completions(
        &self,
        request: &ChatCompletionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_chat_request(request, context)?;
        self.execute_json_request(request).await
    }

    async fn chat_completions_stream(
        &self,
        _request: &ChatCompletionsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented(
            "streaming adapter is deferred to the next phase".to_string(),
        ))
    }

    async fn embeddings(
        &self,
        request: &EmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_embeddings_request(request, context)?;
        self.execute_json_request(request).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::{Json, Router, http::StatusCode, routing::post};
    use gateway_core::{
        ChatCompletionsRequest, ProviderClient, ProviderError, ProviderRequestContext,
        protocol::openai::ChatMessage,
    };
    use serde_json::{Map, Value, json};
    use tokio::net::TcpListener;

    use super::{OpenAiCompatConfig, OpenAiCompatProvider};

    #[test]
    fn builds_openai_chat_request_with_expected_headers_and_body() {
        let mut default_headers = BTreeMap::new();
        default_headers.insert("x-team".to_string(), "gateway".to_string());

        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openai-prod".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            bearer_token: Some("test-token".to_string()),
            default_headers,
            request_timeout_ms: 10_000,
        })
        .expect("provider");

        let request = ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: Value::String("ping".to_string()),
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra: BTreeMap::new(),
        };

        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            idempotency_key: None,
            request_headers: BTreeMap::new(),
        };

        let built = provider
            .build_chat_request(&request, &context)
            .expect("build request");

        assert_eq!(built.method(), reqwest::Method::POST);
        assert_eq!(
            built.url().as_str(),
            "https://api.openai.com/v1/chat/completions"
        );

        let headers = built.headers();
        assert_eq!(
            headers.get("x-team").and_then(|value| value.to_str().ok()),
            Some("gateway")
        );
        assert_eq!(
            headers
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
            Some("req-123")
        );
        assert!(headers.get("authorization").is_some());

        let body = built
            .body()
            .and_then(|body| body.as_bytes())
            .expect("bytes body");
        let body_json: Value = serde_json::from_slice(body).expect("json body");
        assert_eq!(body_json["model"], "gpt-4o-mini");
    }

    #[tokio::test]
    async fn maps_upstream_http_errors() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({"error": "rate_limited"})),
                )
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");

        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve app");
        });

        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openai-prod".to_string(),
            base_url: format!("http://{addr}/v1"),
            bearer_token: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
        })
        .expect("provider");

        let request = ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: false,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            idempotency_key: None,
            request_headers: BTreeMap::new(),
        };

        let error = provider
            .chat_completions(&request, &context)
            .await
            .expect_err("upstream should fail");

        match error {
            ProviderError::UpstreamHttp { status, .. } => assert_eq!(status, 429),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }
}
