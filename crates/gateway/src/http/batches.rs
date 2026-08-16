use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use gateway_core::{
    AdminApiKeyRepository, AuthError, AuthenticatedApiKey, BatchAccessScope, BatchEndpoint,
    BatchItemQuery, BatchItemRecord, BatchItemStatus, BatchJobRecord, BatchPricingStatus,
    BatchQuery, BatchRepository, BatchStatus, GlobalRole, IdentityRepository, Money4,
    extract_bearer_token,
};
use gateway_service::{CreateBatchInput, CreateBatchItemInput};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_active_session, admin_contract::format_timestamp, error::AppError,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct CreateBatchRequest {
    pub endpoint: BatchEndpoint,
    pub model: String,
    pub items: Vec<CreateBatchRequestItem>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBatchRequestItem {
    pub custom_id: String,
    pub body: Value,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListBatchesQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub status: Option<BatchStatus>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub user_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub created_at_start: Option<String>,
    pub created_at_end: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct BatchResultsQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub status: Option<BatchItemStatus>,
    pub format: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchCallerResponse {
    pub api_key_id: Uuid,
    pub api_key_name: Option<String>,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub service_account_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchResponse {
    pub batch_id: Uuid,
    pub status: BatchStatus,
    pub endpoint: BatchEndpoint,
    pub model: String,
    pub resolved_model: String,
    pub upstream_model: String,
    pub provider: String,
    pub route_id: Uuid,
    pub provider_batch_id: Option<String>,
    pub caller: BatchCallerResponse,
    pub request_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub cost_usd: Option<f64>,
    pub pricing_status: BatchPricingStatus,
    pub provider_usage: Option<Value>,
    pub error: Option<Value>,
    pub created_at: String,
    pub submitted_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct BatchListResponse {
    pub items: Vec<BatchResponse>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Serialize)]
pub struct BatchResultResponse {
    pub custom_id: String,
    pub status: BatchItemStatus,
    pub request: Value,
    pub response: Option<Value>,
    pub error: Option<Value>,
    pub provider_request_id: Option<String>,
    pub provider_usage: Option<Value>,
    pub cost_usd: Option<f64>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchResultsResponse {
    pub batch: BatchResponse,
    pub items: Vec<BatchResultResponse>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

pub async fn create_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateBatchRequest>,
) -> Result<Response, AppError> {
    let auth = require_api_key(&state, &headers).await?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            AppError(gateway_core::GatewayError::InvalidRequest(
                "Idempotency-Key header is required".to_string(),
            ))
        })?;
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let job = gateway_service::create_batch(
        state.service.as_ref(),
        &state.providers,
        &auth,
        CreateBatchInput {
            idempotency_key: idempotency_key.to_string(),
            request_id,
            endpoint: request.endpoint,
            model: request.model,
            items: request
                .items
                .into_iter()
                .map(|item| CreateBatchItemInput {
                    custom_id: item.custom_id,
                    body: item.body,
                })
                .collect(),
        },
    )
    .await?;
    let response = batch_response(&state, job).await?;
    Ok((StatusCode::ACCEPTED, Json(response)).into_response())
}

pub async fn list_batches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListBatchesQuery>,
) -> Result<Json<BatchListResponse>, AppError> {
    let scope = access_scope(&state, &headers).await?;
    let page = state
        .store
        .list_batches(
            &BatchQuery {
                page: query.page.unwrap_or(1),
                page_size: query.page_size.unwrap_or(50),
                status: query.status,
                model_key: query.model,
                provider_key: query.provider,
                user_id: query.user_id,
                service_account_id: query.service_account_id,
                created_at_start: parse_time(query.created_at_start.as_deref())?,
                created_at_end: parse_time(query.created_at_end.as_deref())?,
            },
            scope,
        )
        .await?;
    let mut items = Vec::with_capacity(page.items.len());
    for job in page.items {
        items.push(batch_response(&state, job).await?);
    }
    Ok(Json(BatchListResponse {
        items,
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    }))
}

pub async fn get_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(batch_id): Path<Uuid>,
) -> Result<Json<BatchResponse>, AppError> {
    let scope = access_scope(&state, &headers).await?;
    let job = state.store.get_batch(batch_id, scope).await?;
    Ok(Json(batch_response(&state, job).await?))
}

pub async fn get_batch_results(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(batch_id): Path<Uuid>,
    Query(query): Query<BatchResultsQuery>,
) -> Result<Response, AppError> {
    let scope = access_scope(&state, &headers).await?;
    let job = state.store.get_batch(batch_id, scope).await?;
    let page = state
        .store
        .list_batch_items(
            batch_id,
            &BatchItemQuery {
                page: query.page.unwrap_or(1),
                page_size: query.page_size.unwrap_or(100),
                status: query.status,
            },
            scope,
        )
        .await?;
    let items = page
        .items
        .into_iter()
        .map(result_response)
        .collect::<Vec<_>>();
    if query.format.as_deref() == Some("jsonl") {
        let mut body = String::new();
        for item in &items {
            body.push_str(&serde_json::to_string(item).map_err(|error| {
                AppError(gateway_core::GatewayError::Internal(error.to_string()))
            })?);
            body.push('\n');
        }
        return Ok(([(CONTENT_TYPE, "application/x-ndjson")], Body::from(body)).into_response());
    }
    Ok(Json(BatchResultsResponse {
        batch: batch_response(&state, job).await?,
        items,
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })
    .into_response())
}

pub async fn cancel_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(batch_id): Path<Uuid>,
) -> Result<Json<BatchResponse>, AppError> {
    let scope = access_scope(&state, &headers).await?;
    let current = state.store.get_batch(batch_id, scope).await?;
    if !matches!(
        current.status,
        BatchStatus::Queued | BatchStatus::Submitting
    ) && !current.status.is_terminal()
    {
        let provider = state.providers.get(&current.provider_key).ok_or_else(|| {
            AppError(gateway_core::GatewayError::Internal(format!(
                "batch provider `{}` is not registered",
                current.provider_key
            )))
        })?;
        if !provider.batch_capabilities().cancel {
            return Err(AppError(gateway_core::GatewayError::NotImplemented(
                "the selected provider does not support batch cancellation".to_string(),
            )));
        }
    }
    let job = state
        .store
        .request_batch_cancel(batch_id, scope, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(batch_response(&state, job).await?))
}

async fn access_scope(state: &AppState, headers: &HeaderMap) -> Result<BatchAccessScope, AppError> {
    if let Some(auth) = optional_api_key(state, headers).await? {
        return Ok(BatchAccessScope::ApiKey(auth.id));
    }
    let user = require_active_session(state, headers).await?;
    Ok(if user.global_role == GlobalRole::PlatformAdmin {
        BatchAccessScope::All
    } else {
        BatchAccessScope::User(user.user_id)
    })
}

async fn require_api_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedApiKey, AppError> {
    optional_api_key(state, headers).await?.ok_or({
        AppError(gateway_core::GatewayError::Auth(
            AuthError::MissingBearerToken,
        ))
    })
}

async fn optional_api_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<AuthenticatedApiKey>, AppError> {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok());
    let explicit = headers
        .get("x-oceans-api-key")
        .and_then(|value| value.to_str().ok());
    let bearer = authorization.map(extract_bearer_token).transpose()?;
    let token = match (bearer, explicit) {
        (Some(left), Some(right)) if left != right => {
            return Err(AppError(gateway_core::GatewayError::Auth(
                AuthError::ConflictingApiKeyHeaders,
            )));
        }
        (Some(token), _) | (None, Some(token)) => Some(token),
        (None, None) => None,
    };
    match token {
        Some(token) => state
            .service
            .authenticate_bearer_token(token)
            .await
            .map(Some)
            .map_err(AppError),
        None => Ok(None),
    }
}

async fn batch_response(state: &AppState, job: BatchJobRecord) -> Result<BatchResponse, AppError> {
    let api_key_name = state
        .store
        .get_api_key_by_id(job.api_key_id)
        .await?
        .map(|key| key.name);
    let user_name = match job.user_id {
        Some(user_id) => state
            .store
            .get_user_by_id(user_id)
            .await?
            .map(|user| user.name),
        None => None,
    };
    let service_account_name = match job.service_account_id {
        Some(id) => state
            .store
            .get_service_account_by_id(id)
            .await?
            .map(|account| account.service_account_name),
        None => None,
    };
    Ok(BatchResponse {
        batch_id: job.batch_id,
        status: job.status,
        endpoint: job.endpoint,
        model: job.model_key,
        resolved_model: job.resolved_model_key,
        upstream_model: job.upstream_model,
        provider: job.provider_key,
        route_id: job.route_id,
        provider_batch_id: job.provider_batch_id,
        caller: BatchCallerResponse {
            api_key_id: job.api_key_id,
            api_key_name,
            user_id: job.user_id,
            user_name,
            team_id: job.team_id,
            service_account_id: job.service_account_id,
            service_account_name,
        },
        request_count: job.request_count,
        completed_count: job.completed_count,
        failed_count: job.failed_count,
        cost_usd: money(job.cost_usd),
        pricing_status: job.pricing_status,
        provider_usage: job.provider_usage,
        error: job.error,
        created_at: format_timestamp(job.created_at),
        submitted_at: job.submitted_at.map(format_timestamp),
        completed_at: job.completed_at.map(format_timestamp),
        updated_at: format_timestamp(job.updated_at),
    })
}

fn result_response(item: BatchItemRecord) -> BatchResultResponse {
    BatchResultResponse {
        custom_id: item.custom_id,
        status: item.status,
        request: item.request_body,
        response: item.response_body,
        error: item.error,
        provider_request_id: item.provider_request_id,
        provider_usage: item.provider_usage,
        cost_usd: money(item.cost_usd),
        completed_at: item.completed_at.map(format_timestamp),
    }
}

fn parse_time(raw: Option<&str>) -> Result<Option<OffsetDateTime>, AppError> {
    raw.map(|value| {
        OffsetDateTime::parse(value, &Rfc3339).map_err(|error| {
            AppError(gateway_core::GatewayError::InvalidRequest(format!(
                "invalid RFC 3339 date `{value}`: {error}"
            )))
        })
    })
    .transpose()
}

fn money(value: Option<Money4>) -> Option<f64> {
    value.map(|money| money.as_scaled_i64() as f64 / Money4::SCALE as f64)
}
