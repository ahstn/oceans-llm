use gateway_core::{CoreDecisionsRequest, ProviderError, ProviderRequestContext};
use serde_json::Value;

use super::{OpenAiCompatConfig, OpenAiCompatProvider};
use crate::decisions::{
    base_decisions_request_body, base_root_and_host, decisions_request_builder,
    validate_decisions_response,
};
use crate::http::{join_base_url, map_reqwest_error};

impl OpenAiCompatProvider {
    pub(super) async fn decisions_impl(
        &self,
        request: &CoreDecisionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let token = self.auth_token().await?;
        let http_request =
            self.build_decisions_request_with_token(request, context, token.as_deref())?;
        let value = self.execute_json_request(http_request).await?;
        validate_decisions_response(&value, request)?;
        Ok(value)
    }

    fn build_decisions_request_with_token(
        &self,
        request: &CoreDecisionsRequest,
        context: &ProviderRequestContext,
        bearer_token: Option<&str>,
    ) -> Result<reqwest::Request, ProviderError> {
        let body = decisions_request_body(request, context)?;

        let url = decisions_url(&self.config)?;

        let mut builder = decisions_request_builder(
            &self.client,
            url,
            &body,
            &self.config.default_headers,
            context,
        );
        if let Some(token) = bearer_token {
            builder = self.config.bearer_auth_header.apply(builder, token);
        }
        builder.build().map_err(map_reqwest_error)
    }
}

fn decisions_request_body(
    request: &CoreDecisionsRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut body = base_decisions_request_body(request, context)?;
    super::apply_openrouter_routing_policy(&mut body, context)?;
    Ok(body)
}

fn decisions_url(config: &OpenAiCompatConfig) -> Result<String, ProviderError> {
    match config.decisions_url.as_deref() {
        Some(url) => Ok(join_base_url(url.trim_end_matches('/'), "")?
            .trim_end_matches('/')
            .to_string()),
        None => decisions_url_for_base(&config.base_url),
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
        CoreDecisionQuestion, OpenRouterProviderRouting, OpenRouterRouteApi, ProviderClient,
        ProviderError,
    };
    use serde_json::{Map, Value, json};
    use tokio::net::TcpListener;

    use super::super::{OpenAiBatchConfig, OpenAiCompatConfig};
    use super::*;

    fn request() -> CoreDecisionsRequest {
        CoreDecisionsRequest {
            model: "jev".to_string(),
            state: json!("Help! My payouts have been failing for 3 days."),
            questions: BTreeMap::from([
                (
                    "is_urgent".to_string(),
                    CoreDecisionQuestion::Noul {
                        instructions: json!("Does this convey urgency?"),
                        criteria: None,
                    },
                ),
                (
                    "department".to_string(),
                    CoreDecisionQuestion::Choice {
                        instructions: json!("Which team?"),
                        criteria: BTreeMap::from([
                            ("billing".to_string(), Some(json!("Payments"))),
                            ("technical".to_string(), None),
                        ]),
                    },
                ),
                (
                    "frustration".to_string(),
                    CoreDecisionQuestion::Score {
                        instructions: json!("How frustrated?"),
                        criteria: vec![json!("Calm"), json!("Frustrated")],
                    },
                ),
            ]),
            extra: BTreeMap::new(),
        }
    }

    fn context() -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-decisions".to_string(),
            model_key: "jev".to_string(),
            provider_key: "openrouter".to_string(),
            upstream_model: "typesafe/jev-1.13".to_string(),
            owner_user_id: None,
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
        let body = decisions_request_body(&request(), &context()).expect("body");
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
        let body = decisions_request_body(&request(), &no_policy).expect("body");
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
        let value = provider
            .decisions(&request(), &context())
            .await
            .expect("decisions");
        assert_eq!(value["answers"]["is_urgent"]["noul"], json!(0.95));
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
        assert!(matches!(error, ProviderError::Transport(_)), "{error:?}");
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
}
