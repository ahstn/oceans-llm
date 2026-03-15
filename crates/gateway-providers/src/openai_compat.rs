use std::{collections::BTreeMap, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream, core_chat_request_to_openai,
    core_embeddings_request_to_openai,
};
use serde_json::Value;

use crate::http::{join_base_url, map_reqwest_error};
use crate::streaming::{
    SseEventParser, done_sse_chunk, openai_sse_error_chunk, render_sse_event_chunk,
};

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
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let wire_request = core_chat_request_to_openai(request);
        let mut body = serde_json::to_value(wire_request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        self.build_request("chat/completions", body, context, false)
    }

    pub fn build_chat_stream_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let mut stream_request = request.clone();
        stream_request.stream = true;
        let wire_request = core_chat_request_to_openai(&stream_request);
        let mut body = serde_json::to_value(wire_request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        self.build_request("chat/completions", body, context, true)
    }

    pub fn build_embeddings_request(
        &self,
        request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let wire_request = core_embeddings_request_to_openai(request);
        let mut body = serde_json::to_value(wire_request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        self.build_request("embeddings", body, context, false)
    }

    fn build_request(
        &self,
        endpoint_suffix: &str,
        mut body: Value,
        context: &ProviderRequestContext,
        enforce_stream: bool,
    ) -> Result<reqwest::Request, ProviderError> {
        if let Some(object) = body.as_object_mut() {
            for (key, value) in &context.extra_body {
                object.insert(key.clone(), value.clone());
            }
            if enforce_stream {
                object.insert("stream".to_string(), Value::Bool(true));
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

    async fn execute_stream_request(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response, ProviderError> {
        let response = self
            .client
            .execute(request)
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(map_reqwest_error)?;
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body,
            });
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if !content_type
            .as_deref()
            .is_some_and(is_event_stream_content_type)
        {
            let rendered = content_type.unwrap_or_else(|| "<missing>".to_string());
            return Err(ProviderError::Transport(format!(
                "openai_compat stream response content-type must be `text/event-stream`, got `{rendered}`"
            )));
        }

        Ok(response)
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
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_chat_request(request, context)?;
        self.execute_json_request(request).await
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let request = self.build_chat_stream_request(request, context)?;
        let response = self.execute_stream_request(request).await?;

        Ok(normalize_openai_compat_stream(response.bytes_stream()))
    }

    async fn embeddings(
        &self,
        request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_embeddings_request(request, context)?;
        self.execute_json_request(request).await
    }
}

fn normalize_openai_compat_stream<S>(upstream: S) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_openai_compat_stream_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "openai_compat_sse_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                let data = event.data.trim();
                if data == "[DONE]" {
                    continue;
                }

                if data.is_empty() && event.event.is_none() {
                    continue;
                }

                saw_payload_event = true;
                yield Ok(render_sse_event_chunk(event.event.as_deref(), &event.data));
            }
        }

        if !stream_failed && let Err(error) = parser.finish() {
            yield Ok(openai_sse_error_chunk(
                "openai_compat_sse_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !saw_payload_event {
            yield Ok(openai_sse_error_chunk(
                "openai_compat_empty_stream",
                "upstream stream ended without SSE payload events",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

fn is_event_stream_content_type(value: &str) -> bool {
    value
        .split(';')
        .next()
        .map(str::trim)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("text/event-stream"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::{
        Json, Router,
        body::Body,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::post,
    };
    use bytes::Bytes;
    use futures_util::{StreamExt, stream};
    use gateway_core::{
        CoreChatMessage, CoreChatRequest, ProviderClient, ProviderError, ProviderRequestContext,
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![CoreChatMessage {
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

    #[test]
    fn build_chat_stream_request_enforces_stream_true_after_overrides() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openai-prod".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            bearer_token: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
        })
        .expect("provider");

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("ping".to_string()),
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra: BTreeMap::new(),
        };

        let mut extra_body = Map::new();
        extra_body.insert("stream".to_string(), Value::Bool(false));
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body,
            request_headers: BTreeMap::new(),
        };

        let built = provider
            .build_chat_stream_request(&request, &context)
            .expect("build request");
        let body = built
            .body()
            .and_then(|body| body.as_bytes())
            .expect("bytes body");
        let body_json: Value = serde_json::from_slice(body).expect("json body");
        assert_eq!(body_json["stream"], Value::Bool(true));
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

        let request = CoreChatRequest {
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

    #[tokio::test]
    async fn streams_openai_compat_sse_transcript() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        "data:{\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n\
                         data: [DONE]\n\n",
                    ))
                    .expect("response")
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let mut stream = provider
            .chat_completions_stream(&request, &context)
            .await
            .expect("stream");

        let mut rendered = String::new();
        while let Some(chunk) = stream.next().await {
            rendered.push_str(std::str::from_utf8(chunk.expect("chunk").as_ref()).expect("utf8"));
        }

        assert!(rendered.contains("\"content\":\"hi\""));
        assert!(rendered.contains("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn appends_done_when_upstream_omits_done_marker() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                    ))
                    .expect("response")
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let mut stream = provider
            .chat_completions_stream(&request, &context)
            .await
            .expect("stream");

        let mut rendered = String::new();
        while let Some(chunk) = stream.next().await {
            rendered.push_str(std::str::from_utf8(chunk.expect("chunk").as_ref()).expect("utf8"));
        }

        assert_eq!(rendered.matches("data: [DONE]").count(), 1);
    }

    #[tokio::test]
    async fn stream_maps_upstream_http_errors() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error":"temporarily_unavailable"})),
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let error = match provider.chat_completions_stream(&request, &context).await {
            Err(error) => error,
            Ok(_) => panic!("stream should fail"),
        };

        match error {
            ProviderError::UpstreamHttp { status, .. } => assert_eq!(status, 503),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_emits_error_chunk_on_midstream_parse_failure() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                let chunks = stream::iter(vec![
                    Ok::<Bytes, std::io::Error>(Bytes::from_static(
                        b"data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                    )),
                    Ok(Bytes::from_static(&[0xff])),
                ]);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(chunks))
                    .expect("response")
                    .into_response()
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let mut stream = provider
            .chat_completions_stream(&request, &context)
            .await
            .expect("stream");

        let mut rendered = String::new();
        while let Some(chunk) = stream.next().await {
            rendered.push_str(std::str::from_utf8(chunk.expect("chunk").as_ref()).expect("utf8"));
        }

        assert!(rendered.contains("\"code\":\"openai_compat_sse_parse_error\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn stream_rejects_non_event_stream_content_type() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from("{\"ok\":true}"))
                    .expect("response")
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let error = match provider.chat_completions_stream(&request, &context).await {
            Err(error) => error,
            Ok(_) => panic!("stream should fail"),
        };
        match error {
            ProviderError::Transport(message) => {
                assert!(message.contains("text/event-stream"));
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_emits_error_chunk_on_incomplete_final_event() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
                    ))
                    .expect("response")
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let mut stream = provider
            .chat_completions_stream(&request, &context)
            .await
            .expect("stream");

        let mut rendered = String::new();
        while let Some(chunk) = stream.next().await {
            rendered.push_str(std::str::from_utf8(chunk.expect("chunk").as_ref()).expect("utf8"));
        }

        assert!(rendered.contains("\"code\":\"openai_compat_sse_finalization_error\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn stream_rejects_done_only_transcript_as_empty_stream() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from("data: [DONE]\n\n"))
                    .expect("response")
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

        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![],
            stream: true,
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-123".to_string(),
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
        };

        let mut stream = provider
            .chat_completions_stream(&request, &context)
            .await
            .expect("stream");

        let mut rendered = String::new();
        while let Some(chunk) = stream.next().await {
            rendered.push_str(std::str::from_utf8(chunk.expect("chunk").as_ref()).expect("utf8"));
        }

        assert!(rendered.contains("\"code\":\"openai_compat_empty_stream\""));
        assert!(!rendered.contains("data: [DONE]"));
    }
}
