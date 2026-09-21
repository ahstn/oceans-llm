use std::collections::BTreeMap;

use async_trait::async_trait;
use gateway_core::{
    CoreChatRequest, CoreDecisionsRequest, CoreEmbeddingsRequest, CoreResponsesRequest,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
};
use serde_json::Value;

use crate::decisions::{
    base_decisions_request_body, base_root_and_host, decisions_request_builder,
    validate_decisions_response,
};
use crate::http::{execute_json_request, join_base_url, map_reqwest_error};

/// Default TypeSafe API root. The Decisions path is `POST /v1/systemone`.
pub const DEFAULT_TYPESAFE_BASE_URL: &str = "https://api.typesafe.ai";

#[derive(Debug, Clone)]
pub struct TypeSafeConfig {
    pub provider_key: String,
    pub base_url: String,
    pub bearer_token: Option<String>,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

impl TypeSafeConfig {
    #[must_use]
    pub fn new(provider_key: String, base_url: String) -> Self {
        Self {
            provider_key,
            base_url,
            bearer_token: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: crate::DEFAULT_REQUEST_TIMEOUT_MS,
        }
    }
}

pub struct TypeSafeProvider {
    config: TypeSafeConfig,
    client: reqwest::Client,
}

impl TypeSafeProvider {
    pub fn new(config: TypeSafeConfig) -> Result<Self, ProviderError> {
        let client = crate::http::provider_http_client(config.request_timeout_ms)?;
        Ok(Self { config, client })
    }

    fn systemone_url(&self) -> Result<String, ProviderError> {
        systemone_url_for_base(&self.config.base_url)
    }

    fn build_decisions_request(
        &self,
        request: &CoreDecisionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let body = base_decisions_request_body(request, context)?;
        let mut builder = decisions_request_builder(
            &self.client,
            self.systemone_url()?,
            &body,
            &self.config.default_headers,
            context,
        );
        if let Some(token) = self.config.bearer_token.as_deref() {
            builder = builder.bearer_auth(token);
        }
        builder.build().map_err(map_reqwest_error)
    }
}

/// Native TypeSafe endpoint. A `/v1` root appends `systemone`, a bare root
/// appends `v1/systemone`, so both `https://api.typesafe.ai` and
/// `https://api.typesafe.ai/v1` configure correctly.
fn systemone_url_for_base(base_url: &str) -> Result<String, ProviderError> {
    let (trimmed, _) = base_root_and_host(base_url)?;
    let parsed = url::Url::parse(&trimmed)
        .map_err(|error| ProviderError::Transport(format!("invalid base_url: {error}")))?;
    if !matches!(parsed.path(), "" | "/" | "/v1" | "/v1/") {
        return Err(ProviderError::InvalidRequest(
            "typesafe base_url path must be empty, `/`, or `/v1`".to_string(),
        ));
    }
    let suffix = if trimmed.ends_with("/v1") {
        "systemone"
    } else {
        "v1/systemone"
    };
    join_base_url(&trimmed, suffix)
}

#[async_trait]
impl ProviderClient for TypeSafeProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "typesafe"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completions: false,
            responses: false,
            stream: false,
            embeddings: false,
            decisions: true,
            tools: false,
            vision: false,
            json_schema: false,
            developer_role: false,
        }
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "typesafe does not support chat completions".to_string(),
        ))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented(
            "typesafe does not support chat completions".to_string(),
        ))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "typesafe does not support embeddings".to_string(),
        ))
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "typesafe does not support responses".to_string(),
        ))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented(
            "typesafe does not support responses".to_string(),
        ))
    }

    async fn decisions(
        &self,
        request: &CoreDecisionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let http_request = self.build_decisions_request(request, context)?;
        let value = execute_json_request(
            &self.client,
            http_request,
            "typesafe",
            &self.config.provider_key,
        )
        .await?;
        validate_decisions_response(&value, request)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
    use gateway_core::CoreDecisionQuestion;
    use serde_json::{Map, Value, json};
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn systemone_url_accepts_bare_and_versioned_roots() {
        assert_eq!(
            systemone_url_for_base("https://api.typesafe.ai").expect("bare root"),
            "https://api.typesafe.ai/v1/systemone"
        );
        assert_eq!(
            systemone_url_for_base("https://api.typesafe.ai/v1/").expect("versioned root"),
            "https://api.typesafe.ai/v1/systemone"
        );
        assert!(systemone_url_for_base("https://api.typesafe.ai/custom").is_err());
    }

    #[test]
    fn decisions_body_uses_upstream_model_and_extra_body() {
        let provider = TypeSafeProvider::new(TypeSafeConfig {
            provider_key: "typesafe".to_string(),
            base_url: "https://api.typesafe.ai".to_string(),
            bearer_token: Some("secret".to_string()),
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
        })
        .expect("provider");
        let request = CoreDecisionsRequest {
            model: "jev".to_string(),
            state: json!("state"),
            questions: BTreeMap::from([(
                "is_urgent".to_string(),
                CoreDecisionQuestion::Noul {
                    instructions: json!("urgent?"),
                    criteria: None,
                },
            )]),
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-1".to_string(),
            model_key: "jev".to_string(),
            provider_key: "typesafe".to_string(),
            upstream_model: "jev-latest".to_string(),
            owner_user_id: None,
            extra_headers: Map::new(),
            extra_body: Map::from_iter([("trace".to_string(), json!(true))]),
            request_headers: BTreeMap::new(),
            compatibility: Default::default(),
        };
        let http_request = provider
            .build_decisions_request(&request, &context)
            .expect("request");
        assert_eq!(
            http_request.url().as_str(),
            "https://api.typesafe.ai/v1/systemone"
        );
        let body: Value = serde_json::from_slice(
            http_request
                .body()
                .and_then(|body| body.as_bytes())
                .expect("json body"),
        )
        .expect("parse body");
        assert_eq!(body["model"], json!("jev-latest"));
        assert_eq!(body["trace"], json!(true));
    }

    #[tokio::test]
    async fn decisions_round_trip_posts_systemone_with_bearer_auth() {
        let app = Router::new().route(
            "/v1/systemone",
            post(|request: axum::extract::Request| async move {
                let (parts, body) = request.into_parts();
                if parts
                    .headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    != Some("Bearer secret")
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let body = axum::body::to_bytes(body, 1024 * 1024)
                    .await
                    .expect("read body");
                let body: Value = serde_json::from_slice(&body).expect("json body");
                assert_eq!(body["model"], json!("jev-latest"));
                Json(json!({
                    "model": "jev-1.13.0",
                    "answers": {"is_urgent": {"type": "noul", "noul": 0.9}},
                    "usage": {"input_tokens": 10, "output_tokens": 20}
                }))
                .into_response()
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let provider = TypeSafeProvider::new(TypeSafeConfig {
            provider_key: "typesafe".to_string(),
            base_url: format!("http://{addr}"),
            bearer_token: Some("secret".to_string()),
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
        })
        .expect("provider");
        let request = CoreDecisionsRequest {
            model: "jev".to_string(),
            state: json!("state"),
            questions: BTreeMap::from([(
                "is_urgent".to_string(),
                CoreDecisionQuestion::Noul {
                    instructions: json!("urgent?"),
                    criteria: None,
                },
            )]),
            extra: BTreeMap::new(),
        };
        let context = ProviderRequestContext {
            request_id: "req-1".to_string(),
            model_key: "jev".to_string(),
            provider_key: "typesafe".to_string(),
            upstream_model: "jev-latest".to_string(),
            owner_user_id: None,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
            compatibility: Default::default(),
        };
        let value = provider
            .decisions(&request, &context)
            .await
            .expect("decisions");
        assert_eq!(value["answers"]["is_urgent"]["noul"], json!(0.9));
    }
}
