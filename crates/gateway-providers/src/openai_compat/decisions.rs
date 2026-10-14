use gateway_core::{
    CoreDecisionsRequest, DecisionsWireFormat, OpenAiCompatDecisionsApi, ProviderDecisionsResponse,
    ProviderError, ProviderRequestContext,
};
use serde_json::Value;

use super::{OpenAiCompatConfig, OpenAiCompatProvider};
use crate::decisions::{
    base_decisions_request_body, base_root_and_host, decisions_request_builder,
    parse_decisions_response,
};
use crate::http::{join_base_url, map_reqwest_error};

impl OpenAiCompatProvider {
    pub(super) async fn decisions_impl(
        &self,
        format: DecisionsWireFormat,
        request: &CoreDecisionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderDecisionsResponse, ProviderError> {
        // Translate before auth so unsupported inputs never reach the upstream.
        let body = decisions_request_body(format, request, context)?;
        let token = self.auth_token().await?;
        let http_request =
            self.build_decisions_request_with_token(format, &body, context, token.as_deref())?;
        let value = self.execute_json_request(http_request).await?;
        parse_decisions_response(format, value, request)
    }

    fn build_decisions_request_with_token(
        &self,
        format: DecisionsWireFormat,
        body: &Value,
        context: &ProviderRequestContext,
        bearer_token: Option<&str>,
    ) -> Result<reqwest::Request, ProviderError> {
        let url = decisions_url(&self.config, format)?;

        let mut builder = decisions_request_builder(
            &self.client,
            url,
            body,
            &self.config.default_headers,
            context,
        );
        if let Some(token) = bearer_token {
            builder = self.config.bearer_auth_header.apply(builder, token);
        }
        builder.build().map_err(map_reqwest_error)
    }
}

/// Wire format for this route: `compatibility.openai_compat.decisions_api`
/// selects OpenAI Decisions; the alpha route and a configured `decisions_url`
/// speak System One. `None` means the route has no Decisions upstream.
pub(super) fn route_decisions_wire_format(
    config: &OpenAiCompatConfig,
    context: &ProviderRequestContext,
) -> Option<DecisionsWireFormat> {
    let openai_compat = context.compatibility.openai_compat.as_ref();
    if openai_compat.and_then(|compat| compat.decisions_api)
        == Some(OpenAiCompatDecisionsApi::OpenAi)
    {
        return Some(DecisionsWireFormat::OpenAi);
    }
    let alpha_route = context
        .compatibility
        .openrouter
        .as_ref()
        .is_some_and(|routing| routing.api.is_decisions());
    (alpha_route || config.decisions_url.is_some()).then_some(DecisionsWireFormat::SystemOne)
}

fn decisions_request_body(
    format: DecisionsWireFormat,
    request: &CoreDecisionsRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut body = base_decisions_request_body(format, request, context)?;
    if format == DecisionsWireFormat::SystemOne {
        super::apply_openrouter_routing_policy(&mut body, context)?;
    }
    Ok(body)
}

fn decisions_url(
    config: &OpenAiCompatConfig,
    format: DecisionsWireFormat,
) -> Result<String, ProviderError> {
    match (config.decisions_url.as_deref(), format) {
        (Some(url), _) => Ok(join_base_url(url.trim_end_matches('/'), "")?
            .trim_end_matches('/')
            .to_string()),
        // OpenAI SDKs POST `<base_url>/decisions`, with `/v1` in the base.
        (None, DecisionsWireFormat::OpenAi) => {
            let (trimmed, _) = base_root_and_host(&config.base_url)?;
            join_base_url(&trimmed, "decisions")
        }
        (None, DecisionsWireFormat::SystemOne) => decisions_url_for_base(&config.base_url),
    }
}

/// Derive the upstream Decisions URL. OpenRouter `.../api/v1` maps to the
/// alpha `.../api/alpha/decisions` path; every other base appends the stable
/// `/v1/decisions` family path so future providers share one default.
pub(super) fn decisions_url_for_base(base_url: &str) -> Result<String, ProviderError> {
    let (trimmed, host) = base_root_and_host(base_url)?;
    if host.as_deref() == Some("openrouter.ai") {
        let root = trimmed
            .strip_suffix("/api/v1")
            .unwrap_or(&trimmed)
            .trim_end_matches('/');
        return join_base_url(root, "api/alpha/decisions");
    }
    join_base_url(&trimmed, "v1/decisions")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
    use gateway_core::{
        OpenAiCompatRouteCompatibility, OpenRouterProviderRouting, OpenRouterRouteApi,
        ProviderClient, ProviderError,
    };
    use serde_json::{Map, Value, json};
    use tokio::net::TcpListener;

    use super::super::{OpenAiBatchConfig, OpenAiCompatConfig};
    use super::*;

    fn request() -> CoreDecisionsRequest {
        crate::decisions::system_one_test_request()
    }

    fn context() -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-decisions".to_string(),
            model_key: "jev".to_string(),
            provider_key: "openrouter".to_string(),
            upstream_model: "typesafe/jev-1.13".to_string(),
            owner_user_id: None,
            expected_provider_credential_id: None,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
            compatibility: gateway_core::RouteCompatibility {
                openrouter: Some(gateway_core::OpenRouterRouteCompatibility {
                    provider: OpenRouterProviderRouting {
                        order: vec!["typesafe".to_string()],
                        ..Default::default()
                    },
                    api: OpenRouterRouteApi::Decisions,
                }),
                ..Default::default()
            },
        }
    }

    #[test]
    fn derives_openrouter_alpha_url_and_stable_default() {
        assert_eq!(
            decisions_url_for_base("https://openrouter.ai/api/v1").expect("openrouter url"),
            "https://openrouter.ai/api/alpha/decisions"
        );
        assert_eq!(
            decisions_url_for_base("https://openrouter.ai/api/v1/").expect("trailing slash"),
            "https://openrouter.ai/api/alpha/decisions"
        );
        assert_eq!(
            decisions_url_for_base("https://decisions.example.test/base").expect("default"),
            "https://decisions.example.test/base/v1/decisions"
        );
    }

    #[test]
    fn builds_decisions_body_with_routing_policy_and_upstream_model() {
        let body = decisions_request_body(DecisionsWireFormat::SystemOne, &request(), &context())
            .expect("body");
        assert_eq!(body["model"], json!("typesafe/jev-1.13"));
        assert_eq!(
            body["questions"]["department"]["criteria"]["billing"],
            json!("Payments")
        );
        assert_eq!(body["provider"], json!({"order": ["typesafe"]}));
    }

    #[test]
    fn omits_empty_openrouter_provider_policy_from_decisions_body() {
        let mut no_policy = context();
        if let Some(openrouter) = no_policy.compatibility.openrouter.as_mut() {
            openrouter.provider = OpenRouterProviderRouting::default();
        }
        let body = decisions_request_body(DecisionsWireFormat::SystemOne, &request(), &no_policy)
            .expect("body");
        assert!(body.get("provider").is_none());
    }

    fn decisions_provider(decisions_url: String) -> OpenAiCompatProvider {
        OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openrouter".to_string(),
            provider_type: "openai_compat".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            bearer_token: Some("test-token".to_string()),
            bearer_auth_header: super::super::BearerAuthHeader::Authorization,
            identity_token_source: None,
            routing_auth_identity: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
            batch: OpenAiBatchConfig::default(),
            decisions_url: Some(decisions_url),
        })
        .expect("provider")
    }

    #[tokio::test]
    async fn decisions_round_trip_sends_auth_headers_and_validates_answers() {
        let app = Router::new().route(
            "/decisions",
            post(|request: axum::extract::Request| async move {
                let (parts, body) = request.into_parts();
                if parts
                    .headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    != Some("Bearer test-token")
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                if !parts.headers.contains_key("x-request-id") {
                    return StatusCode::BAD_REQUEST.into_response();
                }
                let body = axum::body::to_bytes(body, 1024 * 1024)
                    .await
                    .expect("read body");
                let body: Value = serde_json::from_slice(&body).expect("json body");
                assert_eq!(body["model"], json!("typesafe/jev-1.13"));
                assert_eq!(body["provider"]["order"], json!(["typesafe"]));
                Json(json!({
                    "model": "typesafe/jev-1.13",
                    "answers": {
                        "is_urgent": {"type": "noul", "noul": 0.95},
                        "department": {"type": "choice", "choice": "billing"},
                        "frustration": {"type": "score", "score": 0.8}
                    },
                    "usage": {"input_tokens": 100, "output_tokens": 60}
                }))
                .into_response()
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let provider = decisions_provider(format!("http://{addr}/decisions"));
        let response = provider
            .decisions(&request(), &context())
            .await
            .expect("decisions");
        assert_eq!(response.format, DecisionsWireFormat::SystemOne);
        assert_eq!(response.body["answers"]["is_urgent"]["noul"], json!(0.95));
    }

    #[tokio::test]
    async fn decisions_maps_upstream_errors_and_rejects_malformed_answers() {
        let app = Router::new()
            .route(
                "/bad-request",
                post(|| async move {
                    (StatusCode::UNPROCESSABLE_ENTITY, "bad questions").into_response()
                }),
            )
            .route(
                "/overloaded",
                post(
                    || async move { (StatusCode::TOO_MANY_REQUESTS, "slow down").into_response() },
                ),
            )
            .route(
                "/exhausted",
                post(|| async move {
                    (StatusCode::from_u16(529).expect("529 status"), "overloaded").into_response()
                }),
            )
            .route(
                "/malformed",
                post(|| async move {
                    Json(json!({
                        "model": "typesafe/jev-1.13",
                        "answers": {"is_urgent": {"type": "bogus"}}
                    }))
                    .into_response()
                }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        for (path, status) in [
            ("/bad-request", 422),
            ("/overloaded", 429),
            ("/exhausted", 529),
        ] {
            let provider = decisions_provider(format!("http://{addr}{path}"));
            let error = provider
                .decisions(&request(), &context())
                .await
                .expect_err("upstream error");
            assert!(
                matches!(error, ProviderError::UpstreamHttp { status: actual, .. } if actual == status),
                "unexpected error for {path}: {error:?}"
            );
        }

        let provider = decisions_provider(format!("http://{addr}/malformed"));
        let error = provider
            .decisions(&request(), &context())
            .await
            .expect_err("malformed answers");
        assert!(
            matches!(&error, ProviderError::PartialUsage { source, .. }
                if matches!(source.as_ref(), ProviderError::Transport(_))),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn decisions_rejects_routes_without_decisions_api() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            bearer_token: None,
            bearer_auth_header: super::super::BearerAuthHeader::Authorization,
            identity_token_source: None,
            routing_auth_identity: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
            batch: OpenAiBatchConfig::default(),
            decisions_url: None,
        })
        .expect("provider");
        let mut context = context();
        context.compatibility = Default::default();
        let error = provider
            .decisions(&request(), &context)
            .await
            .expect_err("unsupported route");
        assert!(
            matches!(error, ProviderError::NotImplemented(_)),
            "{error:?}"
        );
    }

    fn openai_provider(base_url: String) -> OpenAiCompatProvider {
        OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            base_url,
            bearer_token: Some("openai-token".to_string()),
            bearer_auth_header: super::super::BearerAuthHeader::Authorization,
            identity_token_source: None,
            routing_auth_identity: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
            batch: OpenAiBatchConfig::default(),
            decisions_url: None,
        })
        .expect("provider")
    }

    fn openai_context() -> ProviderRequestContext {
        let mut context = context();
        context.provider_key = "openai-prod".to_string();
        context.upstream_model = "gpt-6-luna".to_string();
        context.compatibility = gateway_core::RouteCompatibility {
            openai_compat: Some(OpenAiCompatRouteCompatibility {
                decisions_api: Some(OpenAiCompatDecisionsApi::OpenAi),
                ..Default::default()
            }),
            ..Default::default()
        };
        context
    }

    #[test]
    fn selects_wire_format_from_route_compatibility() {
        let provider = openai_provider("https://api.openai.com/v1".to_string());
        assert_eq!(
            route_decisions_wire_format(&provider.config, &openai_context()),
            Some(DecisionsWireFormat::OpenAi)
        );
        assert_eq!(
            route_decisions_wire_format(&provider.config, &context()),
            Some(DecisionsWireFormat::SystemOne)
        );
        let mut plain = context();
        plain.compatibility = Default::default();
        assert_eq!(route_decisions_wire_format(&provider.config, &plain), None);
        assert_eq!(
            decisions_url(&provider.config, DecisionsWireFormat::OpenAi).expect("url"),
            "https://api.openai.com/v1/decisions"
        );
    }

    #[tokio::test]
    async fn openai_decisions_round_trip_posts_openai_format() {
        let app = Router::new().route(
            "/v1/decisions",
            post(|request: axum::extract::Request| async move {
                let (parts, body) = request.into_parts();
                if parts
                    .headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    != Some("Bearer openai-token")
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let body = axum::body::to_bytes(body, 1024 * 1024)
                    .await
                    .expect("read body");
                let body: Value = serde_json::from_slice(&body).expect("json body");
                assert_eq!(body["model"], json!("gpt-6-luna"));
                assert_eq!(
                    body["input"],
                    json!("Help! My payouts have been failing for 3 days.")
                );
                assert!(body.get("state").is_none());
                assert!(body.get("provider").is_none());
                assert_eq!(body["questions"][0]["name"], json!("department"));
                assert_eq!(body["questions"][2]["type"], json!("predicate"));
                Json(json!({
                    "answers": [
                        {
                            "type": "choice",
                            "name": "department",
                            "choice": "billing",
                            "confidence": 0.9,
                            "probabilities": [
                                {"value": "billing", "probability": 0.9},
                                {"value": "technical", "probability": 0.1}
                            ]
                        },
                        {
                            "type": "score",
                            "name": "frustration",
                            "score": 1.0,
                            "confidence": 0.8,
                            "probabilities": [
                                {"value": 0, "label": "Calm", "probability": 0.2},
                                {"value": 1, "label": "Frustrated", "probability": 0.8}
                            ]
                        },
                        {"type": "predicate", "name": "is_urgent", "probability": 0.95}
                    ],
                    "model": "gpt-6-luna",
                    "usage": {
                        "input_tokens": 200,
                        "input_tokens_details": {"cached_tokens": 150, "cache_write_tokens": 0},
                        "output_tokens": 5,
                        "output_tokens_details": {"reasoning_tokens": 3},
                        "total_tokens": 205
                    }
                }))
                .into_response()
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let provider = openai_provider(format!("http://{addr}/v1"));
        let response = provider
            .decisions(&request(), &openai_context())
            .await
            .expect("decisions");
        assert_eq!(response.format, DecisionsWireFormat::OpenAi);
        let usage = response.parsed.usage.expect("usage");
        assert_eq!(usage.cached_tokens, 150);
        assert_eq!(usage.reasoning_tokens, 3);

        // System One callers get their own shape back.
        let rendered =
            gateway_core::core_decisions_response_to_system_one(&response.parsed, &request());
        assert_eq!(
            rendered["answers"]["is_urgent"],
            json!({"type": "noul", "noul": 0.95})
        );
        assert_eq!(
            rendered["answers"]["frustration"]["probabilities"],
            json!({"0": 0.2, "1": 0.8})
        );
    }

    #[tokio::test]
    async fn rejects_inputs_the_upstream_cannot_carry_before_calling_it() {
        // Unroutable address: any upstream call would fail as a transport error.
        let provider = openai_provider("http://127.0.0.1:9/v1".to_string());
        let mut single_choice = request();
        if let Some(question) = single_choice
            .questions
            .iter_mut()
            .find(|question| question.key == "department")
            && let gateway_core::CoreDecisionQuestionKind::Choice { options } = &mut question.kind
        {
            options.truncate(1);
        }
        let error = provider
            .decisions(&single_choice, &openai_context())
            .await
            .expect_err("OpenAI requires two choices");
        assert!(
            matches!(error, ProviderError::InvalidRequest(_)),
            "{error:?}"
        );

        let alpha = decisions_provider("http://127.0.0.1:9/decisions".to_string());
        let mut with_safety_identifier = request();
        with_safety_identifier.safety_identifier = Some("user-123".to_string());
        let error = alpha
            .decisions(&with_safety_identifier, &context())
            .await
            .expect_err("System One cannot carry safety_identifier");
        assert!(
            matches!(error, ProviderError::InvalidRequest(_)),
            "{error:?}"
        );
    }
}
