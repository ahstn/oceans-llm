use std::collections::BTreeMap;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{
        HeaderMap, HeaderValue,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use gateway_core::{
    ChatCompletionsRequest, EmbeddingsRequest, GatewayError, ModelsListResponse, ProviderError,
    ProviderRequestContext, protocol::openai::ModelCard,
};
use serde_json::json;

use crate::http::{error::AppError, state::AppState};

pub async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn readyz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.service.check_readiness().await?;
    Ok(Json(json!({ "status": "ready" })))
}

pub async fn api_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "gateway" }))
}

pub async fn v1_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModelsListResponse>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;

    let models = state.service.list_models_for_api_key(&auth).await?;
    let data = models
        .into_iter()
        .map(|model| ModelCard {
            id: model.model_key,
            object: "model".to_string(),
            created: 0,
            owned_by: "gateway".to_string(),
        })
        .collect::<Vec<_>>();

    Ok(Json(ModelsListResponse {
        object: "list".to_string(),
        data,
    }))
}

pub async fn v1_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionsRequest>,
) -> Result<Response, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let resolved = state.service.resolve_request(&auth, &request.model).await?;

    let idempotency_key = extract_idempotency_key(&headers).map(str::to_string);
    let request_id = extract_request_id(&headers).to_string();
    let request_headers = extract_request_headers(&headers);
    let allow_fallback = !request.stream && idempotency_key.is_some();

    let mut eligible = Vec::new();
    for route in &resolved.routes {
        let Some(provider) = state.providers.get(&route.provider_key) else {
            continue;
        };
        let caps = provider.capabilities();
        if request.stream {
            if caps.chat_completions_stream {
                eligible.push((route.clone(), provider));
            }
        } else if caps.chat_completions {
            eligible.push((route.clone(), provider));
        }
    }

    tracing::info!(
        request_model = %request.model,
        resolved_model = %resolved.model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count = eligible.len(),
        stream = request.stream,
        fallback_allowed = allow_fallback,
        "chat completion request resolved"
    );

    if eligible.is_empty() {
        return Err(AppError(GatewayError::Provider(
            ProviderError::NotImplemented(
                "no registered provider supports chat completions for resolved routes".to_string(),
            ),
        )));
    }

    if request.stream || !allow_fallback {
        let (route, provider) = eligible
            .into_iter()
            .next()
            .expect("eligible routes checked as non-empty");

        let context = build_provider_context(
            &request_id,
            &resolved.model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers,
        );

        if request.stream {
            let stream = provider
                .chat_completions_stream(&request, &context)
                .await
                .map_err(GatewayError::from)?;
            let body_stream =
                stream.map(|item| item.map_err(|error| std::io::Error::other(error.to_string())));

            let mut response = Response::builder()
                .status(axum::http::StatusCode::OK)
                .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
                .header(CACHE_CONTROL, "no-cache")
                .body(Body::from_stream(body_stream))
                .map_err(|error| {
                    AppError(GatewayError::Internal(format!(
                        "failed to build streaming response: {error}"
                    )))
                })?;

            if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
                response
                    .headers_mut()
                    .insert("x-request-id", request_id_header);
            }

            return Ok(response);
        }

        let value = provider
            .chat_completions(&request, &context)
            .await
            .map_err(GatewayError::from)?;
        return Ok(Json(value).into_response());
    }

    let mut first_failure: Option<GatewayError> = None;

    for (route, provider) in eligible {
        let context = build_provider_context(
            &request_id,
            &resolved.model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers.clone(),
        );

        match provider.chat_completions(&request, &context).await {
            Ok(value) => return Ok(Json(value).into_response()),
            Err(error) => {
                tracing::warn!(
                    provider_key = %route.provider_key,
                    request_model = %request.model,
                    retryable = error.is_retryable(),
                    "chat completion attempt failed"
                );

                if !error.is_retryable() {
                    return Err(AppError(error.into()));
                }

                if first_failure.is_none() {
                    first_failure = Some(error.into());
                }
            }
        }
    }

    Err(AppError(first_failure.unwrap_or_else(|| {
        GatewayError::Provider(ProviderError::Transport(
            "all fallback routes failed without a terminal error".to_string(),
        ))
    })))
}

pub async fn v1_embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let resolved = state.service.resolve_request(&auth, &request.model).await?;

    let has_adapter = resolved
        .routes
        .first()
        .and_then(|route| state.providers.get(&route.provider_key))
        .is_some();

    tracing::info!(
        request_model = %request.model,
        resolved_model = %resolved.model.model_key,
        route_count = resolved.routes.len(),
        provider_adapter_available = has_adapter,
        "embeddings request resolved"
    );

    for route in &resolved.routes {
        let Some(provider) = state.providers.get(&route.provider_key) else {
            continue;
        };

        if !provider.capabilities().embeddings {
            return Err(AppError(GatewayError::Provider(
                ProviderError::NotImplemented(format!(
                    "provider `{}` does not implement embeddings in this slice",
                    route.provider_key
                )),
            )));
        }

        return Err(AppError(GatewayError::NotImplemented(
            "embeddings execution is intentionally deferred in this foundation phase".to_string(),
        )));
    }

    Err(AppError(GatewayError::Provider(
        ProviderError::NotImplemented(
            "no registered provider supports embeddings for resolved routes".to_string(),
        ),
    )))
}

fn build_provider_context(
    request_id: &str,
    model_key: &str,
    route: &gateway_core::ModelRoute,
    idempotency_key: Option<String>,
    request_headers: BTreeMap<String, String>,
) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: request_id.to_string(),
        model_key: model_key.to_string(),
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        extra_headers: route.extra_headers.clone(),
        extra_body: route.extra_body.clone(),
        idempotency_key,
        request_headers,
    }
}

fn extract_authorization_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
}

fn extract_idempotency_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
}

fn extract_request_id(headers: &HeaderMap) -> &str {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing-request-id")
}

fn extract_request_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect::<BTreeMap<_, _>>()
}
