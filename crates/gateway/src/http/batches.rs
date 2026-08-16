use std::collections::{BTreeSet, HashMap};

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, TryStreamExt, stream};
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
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_active_session, admin_contract::format_timestamp, error::AppError,
    state::AppState,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateBatchRequest {
    #[schema(value_type = BatchEndpointSchema)]
    pub endpoint: BatchEndpoint,
    pub model: String,
    pub items: Vec<CreateBatchRequestItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateBatchRequestItem {
    pub custom_id: String,
    pub body: Value,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListBatchesQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    #[param(value_type = Option<BatchStatusSchema>)]
    pub status: Option<BatchStatus>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub user_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub created_at_start: Option<String>,
    pub created_at_end: Option<String>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BatchResultsQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    #[param(value_type = Option<BatchItemStatusSchema>)]
    pub status: Option<BatchItemStatus>,
    pub format: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchCallerResponse {
    pub api_key_id: Uuid,
    pub api_key_name: Option<String>,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub service_account_name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchResponse {
    pub batch_id: Uuid,
    #[schema(value_type = BatchStatusSchema)]
    pub status: BatchStatus,
    #[schema(value_type = BatchEndpointSchema)]
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
    #[schema(value_type = BatchPricingStatusSchema)]
    pub pricing_status: BatchPricingStatus,
    pub provider_usage: Option<Value>,
    pub error: Option<Value>,
    pub created_at: String,
    pub submitted_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchListResponse {
    pub items: Vec<BatchResponse>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchResultResponse {
    pub custom_id: String,
    #[schema(value_type = BatchItemStatusSchema)]
    pub status: BatchItemStatus,
    pub request: Value,
    pub response: Option<Value>,
    pub error: Option<Value>,
    pub provider_request_id: Option<String>,
    pub provider_usage: Option<Value>,
    pub cost_usd: Option<f64>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchResultsResponse {
    pub batch: BatchResponse,
    pub items: Vec<BatchResultResponse>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(ToSchema)]
#[schema(rename_all = "snake_case")]
#[allow(dead_code)]
enum BatchEndpointSchema {
    ChatCompletions,
    Responses,
    Embeddings,
}

#[derive(ToSchema)]
#[schema(rename_all = "snake_case")]
#[allow(dead_code)]
enum BatchStatusSchema {
    Queued,
    Submitting,
    SubmissionUnknown,
    Validating,
    InProgress,
    Finalizing,
    Completed,
    Failed,
    Expired,
    CancelRequested,
    Cancelling,
    Cancelled,
}

#[derive(ToSchema)]
#[schema(rename_all = "snake_case")]
#[allow(dead_code)]
enum BatchItemStatusSchema {
    Pending,
    Succeeded,
    Failed,
}

#[derive(ToSchema)]
#[schema(rename_all = "snake_case")]
#[allow(dead_code)]
enum BatchPricingStatusSchema {
    Pending,
    Priced,
    PartiallyPriced,
    Unpriced,
    ProviderReported,
}

#[utoipa::path(
    post,
    path = "/api/v1/batches",
    request_body = CreateBatchRequest,
    params(("Idempotency-Key" = String, Header, description = "Caller supplied idempotency key")),
    responses((status = 202, description = "Batch accepted", body = BatchResponse)),
    security(("gateway_api_key" = [])),
    tag = "batches"
)]
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
    let caller_names = load_caller_names(&state, std::slice::from_ref(&job)).await?;
    let response = batch_response(job, &caller_names);
    Ok((StatusCode::ACCEPTED, Json(response)).into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/batches",
    params(ListBatchesQuery),
    responses((status = 200, description = "Visible batch requests", body = BatchListResponse)),
    security(("session_cookie" = []), ("gateway_api_key" = [])),
    tag = "batches"
)]
pub async fn list_batches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListBatchesQuery>,
) -> Result<Json<BatchListResponse>, AppError> {
    let scope = access_scope(&state, &headers).await?;
    let user_id = scoped_user_filter(scope, query.user_id);
    let page = state
        .store
        .list_batches(
            &BatchQuery {
                page: query.page.unwrap_or(1),
                page_size: query.page_size.unwrap_or(50),
                status: query.status,
                model_key: query.model,
                provider_key: query.provider,
                user_id,
                service_account_id: query.service_account_id,
                created_at_start: parse_time(query.created_at_start.as_deref())?,
                created_at_end: parse_time(query.created_at_end.as_deref())?,
            },
            scope,
        )
        .await?;
    let caller_names = load_caller_names(&state, &page.items).await?;
    let items = page
        .items
        .into_iter()
        .map(|job| batch_response(job, &caller_names))
        .collect();
    Ok(Json(BatchListResponse {
        items,
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/batches/{batch_id}",
    params(("batch_id" = Uuid, Path, description = "Batch identifier")),
    responses((status = 200, description = "Batch request", body = BatchResponse)),
    security(("session_cookie" = []), ("gateway_api_key" = [])),
    tag = "batches"
)]
pub async fn get_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(batch_id): Path<Uuid>,
) -> Result<Json<BatchResponse>, AppError> {
    let scope = access_scope(&state, &headers).await?;
    let job = state.store.get_batch(batch_id, scope).await?;
    let caller_names = load_caller_names(&state, std::slice::from_ref(&job)).await?;
    Ok(Json(batch_response(job, &caller_names)))
}

#[utoipa::path(
    get,
    path = "/api/v1/batches/{batch_id}/results",
    params(("batch_id" = Uuid, Path, description = "Batch identifier"), BatchResultsQuery),
    responses((status = 200, description = "Paged batch results", body = BatchResultsResponse)),
    security(("session_cookie" = []), ("gateway_api_key" = [])),
    tag = "batches"
)]
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
    let caller_names = load_caller_names(&state, std::slice::from_ref(&job)).await?;
    Ok(Json(BatchResultsResponse {
        batch: batch_response(job, &caller_names),
        items,
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })
    .into_response())
}

#[utoipa::path(
    post,
    path = "/api/v1/batches/{batch_id}/cancel",
    params(("batch_id" = Uuid, Path, description = "Batch identifier")),
    responses((status = 200, description = "Updated batch request", body = BatchResponse)),
    security(("session_cookie" = []), ("gateway_api_key" = [])),
    tag = "batches"
)]
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
    let caller_names = load_caller_names(&state, std::slice::from_ref(&job)).await?;
    Ok(Json(batch_response(job, &caller_names)))
}

async fn access_scope(state: &AppState, headers: &HeaderMap) -> Result<BatchAccessScope, AppError> {
    if let Some(auth) = optional_api_key(state, headers).await? {
        return Ok(BatchAccessScope::ApiKey(auth.id));
    }
    let user = require_active_session(state, headers).await?;
    Ok(session_access_scope(user.user_id, user.global_role))
}

fn session_access_scope(user_id: Uuid, global_role: GlobalRole) -> BatchAccessScope {
    if global_role == GlobalRole::PlatformAdmin {
        BatchAccessScope::All
    } else {
        BatchAccessScope::User(user_id)
    }
}

fn scoped_user_filter(scope: BatchAccessScope, requested_user_id: Option<Uuid>) -> Option<Uuid> {
    match scope {
        BatchAccessScope::User(user_id) => Some(user_id),
        BatchAccessScope::All | BatchAccessScope::ApiKey(_) => requested_user_id,
    }
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

#[derive(Default)]
struct CallerNames {
    api_keys: HashMap<Uuid, String>,
    users: HashMap<Uuid, String>,
    service_accounts: HashMap<Uuid, String>,
}

async fn load_caller_names(
    state: &AppState,
    jobs: &[BatchJobRecord],
) -> Result<CallerNames, AppError> {
    let api_key_ids = jobs
        .iter()
        .map(|job| job.api_key_id)
        .collect::<BTreeSet<_>>();
    let user_ids = jobs
        .iter()
        .filter_map(|job| job.user_id)
        .collect::<BTreeSet<_>>();
    let service_account_ids = jobs
        .iter()
        .filter_map(|job| job.service_account_id)
        .collect::<BTreeSet<_>>();
    let (api_keys, users, service_accounts) = tokio::try_join!(
        stream::iter(api_key_ids)
            .map(|id| async move {
                Ok::<_, gateway_core::StoreError>((
                    id,
                    state
                        .store
                        .get_api_key_by_id(id)
                        .await?
                        .map(|record| record.name),
                ))
            })
            .buffer_unordered(32)
            .try_collect::<Vec<_>>(),
        stream::iter(user_ids)
            .map(|id| async move {
                Ok::<_, gateway_core::StoreError>((
                    id,
                    state
                        .store
                        .get_user_by_id(id)
                        .await?
                        .map(|record| record.name),
                ))
            })
            .buffer_unordered(32)
            .try_collect::<Vec<_>>(),
        stream::iter(service_account_ids)
            .map(|id| async move {
                Ok::<_, gateway_core::StoreError>((
                    id,
                    state
                        .store
                        .get_service_account_by_id(id)
                        .await?
                        .map(|record| record.service_account_name),
                ))
            })
            .buffer_unordered(32)
            .try_collect::<Vec<_>>(),
    )?;
    Ok(CallerNames {
        api_keys: api_keys
            .into_iter()
            .filter_map(|(id, name)| name.map(|name| (id, name)))
            .collect(),
        users: users
            .into_iter()
            .filter_map(|(id, name)| name.map(|name| (id, name)))
            .collect(),
        service_accounts: service_accounts
            .into_iter()
            .filter_map(|(id, name)| name.map(|name| (id, name)))
            .collect(),
    })
}

fn batch_response(job: BatchJobRecord, caller_names: &CallerNames) -> BatchResponse {
    let api_key_name = caller_names.api_keys.get(&job.api_key_id).cloned();
    let user_name = job
        .user_id
        .and_then(|id| caller_names.users.get(&id).cloned());
    let service_account_name = job
        .service_account_id
        .and_then(|id| caller_names.service_accounts.get(&id).cloned());
    BatchResponse {
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
    }
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

#[cfg(test)]
mod tests {
    use super::{scoped_user_filter, session_access_scope};
    use gateway_core::{BatchAccessScope, GlobalRole};
    use uuid::Uuid;

    #[test]
    fn platform_admin_session_can_access_all_batches() {
        assert_eq!(
            session_access_scope(Uuid::new_v4(), GlobalRole::PlatformAdmin),
            BatchAccessScope::All
        );
    }

    #[test]
    fn non_platform_session_is_limited_to_its_user() {
        let user_id = Uuid::new_v4();

        assert_eq!(
            session_access_scope(user_id, GlobalRole::User),
            BatchAccessScope::User(user_id)
        );
    }

    #[test]
    fn non_platform_session_cannot_select_another_user() {
        let user_id = Uuid::new_v4();

        assert_eq!(
            scoped_user_filter(BatchAccessScope::User(user_id), Some(Uuid::new_v4()),),
            Some(user_id)
        );
    }
}
