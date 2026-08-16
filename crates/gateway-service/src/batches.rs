use std::collections::{BTreeMap, BTreeSet};

use gateway_core::{
    AuthenticatedApiKey, BatchEndpoint, BatchJobRecord, BatchPricingStatus, BatchRepository,
    BatchStatus, BudgetAlertRepository, BudgetRepository, GatewayError, IdentityRepository,
    McpToolInvocationRepository, ModelRepository, NewBatchItem, NewBatchJob,
    PricingCatalogRepository, ProviderRegistry, ProviderRepository, ProviderRequestContext,
    RequestLogRepository, RoutePlanner, StoreHealth,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::GatewayService;

pub const MAX_BATCH_ITEMS: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBatchInput {
    pub idempotency_key: String,
    pub request_id: String,
    pub endpoint: BatchEndpoint,
    pub model: String,
    pub items: Vec<CreateBatchItemInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBatchItemInput {
    pub custom_id: String,
    pub body: Value,
}

pub async fn create_batch<S, P>(
    service: &GatewayService<S, P>,
    providers: &ProviderRegistry,
    auth: &AuthenticatedApiKey,
    input: CreateBatchInput,
) -> Result<BatchJobRecord, GatewayError>
where
    S: gateway_core::ApiKeyRepository
        + BatchRepository
        + BudgetAlertRepository
        + BudgetRepository
        + ModelRepository
        + IdentityRepository
        + PricingCatalogRepository
        + RequestLogRepository
        + McpToolInvocationRepository
        + ProviderRepository
        + StoreHealth
        + Send
        + Sync
        + 'static,
    P: RoutePlanner + Send + Sync + 'static,
{
    validate_input(&input)?;
    let request_hash = request_hash(&input)?;
    if let Some(existing) = service
        .store()
        .get_batch_by_idempotency_key(auth.id, &input.idempotency_key)
        .await?
    {
        if existing.request_hash == request_hash {
            return Ok(existing);
        }
        return Err(gateway_core::StoreError::Conflict(
            "idempotency key was already used for a different batch request".to_string(),
        )
        .into());
    }

    let resolved = service.resolve_request(auth, &input.model).await?;
    let (route, _) = resolved
        .routes
        .iter()
        .filter_map(|route| {
            providers
                .get(&route.provider_key)
                .map(|provider| (route, provider))
        })
        .find(|(_, provider)| provider.batch_capabilities().supports(input.endpoint))
        .ok_or_else(|| {
            GatewayError::InvalidRequest(format!(
                "no configured route supports {:?} batch requests",
                input.endpoint
            ))
        })?;

    let now = OffsetDateTime::now_utc();
    service
        .enforce_pre_provider_budget(
            auth,
            &input.request_id,
            Some(resolved.selection.execution_model.id),
            Some(route.upstream_model.as_str()),
            now,
        )
        .await?;

    let provider_context = ProviderRequestContext {
        request_id: input.request_id.clone(),
        model_key: resolved.selection.requested_model.model_key.clone(),
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        extra_headers: route.extra_headers.clone(),
        extra_body: route.extra_body.clone(),
        request_headers: BTreeMap::new(),
        compatibility: route.compatibility.clone(),
    };
    let batch_id = Uuid::new_v4();
    let idempotency_key = input.idempotency_key.clone();
    let items = input
        .items
        .into_iter()
        .map(|item| NewBatchItem {
            batch_item_id: Uuid::new_v4(),
            custom_id: item.custom_id,
            request_body: item.body,
        })
        .collect::<Vec<_>>();
    let job = BatchJobRecord {
        batch_id,
        idempotency_key: input.idempotency_key,
        request_hash: request_hash.clone(),
        api_key_id: auth.id,
        user_id: auth.owner_user_id,
        team_id: auth.owner_team_id,
        service_account_id: auth.owner_service_account_id,
        model_id: resolved.selection.execution_model.id,
        model_key: resolved.selection.requested_model.model_key,
        resolved_model_key: resolved.selection.execution_model.model_key,
        route_id: route.id,
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        endpoint: input.endpoint,
        status: BatchStatus::Queued,
        provider_batch_id: None,
        request_count: i64::try_from(items.len()).unwrap_or(i64::MAX),
        completed_count: 0,
        failed_count: 0,
        cost_usd: None,
        pricing_status: BatchPricingStatus::Pending,
        provider_usage: None,
        error: None,
        created_at: now,
        submitted_at: None,
        completed_at: None,
        updated_at: now,
        next_poll_at: Some(now),
        lease_owner: None,
        lease_expires_at: None,
        provider_context,
    };
    match service
        .store()
        .insert_batch(&NewBatchJob { job, items })
        .await
    {
        Ok(job) => Ok(job),
        Err(gateway_core::StoreError::Conflict(_)) => {
            let existing = service
                .store()
                .get_batch_by_idempotency_key(auth.id, &idempotency_key)
                .await?
                .ok_or_else(|| {
                    GatewayError::Internal(
                        "batch idempotency conflict had no stored batch".to_string(),
                    )
                })?;
            if existing.request_hash == request_hash {
                Ok(existing)
            } else {
                Err(gateway_core::StoreError::Conflict(
                    "idempotency key was already used for a different batch request".to_string(),
                )
                .into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_input(input: &CreateBatchInput) -> Result<(), GatewayError> {
    if input.idempotency_key.is_empty() || input.idempotency_key.len() > 200 {
        return Err(GatewayError::InvalidRequest(
            "Idempotency-Key must contain between 1 and 200 characters".to_string(),
        ));
    }
    if input.items.is_empty() || input.items.len() > MAX_BATCH_ITEMS {
        return Err(GatewayError::InvalidRequest(format!(
            "a batch must contain between 1 and {MAX_BATCH_ITEMS} items"
        )));
    }
    let mut custom_ids = BTreeSet::new();
    for item in &input.items {
        if item.custom_id.is_empty() || item.custom_id.len() > 128 {
            return Err(GatewayError::InvalidRequest(
                "batch custom_id must contain between 1 and 128 characters".to_string(),
            ));
        }
        if !custom_ids.insert(item.custom_id.as_str()) {
            return Err(GatewayError::InvalidRequest(format!(
                "batch custom_id `{}` is duplicated",
                item.custom_id
            )));
        }
        let object = item.body.as_object().ok_or_else(|| {
            GatewayError::InvalidRequest(format!(
                "batch item `{}` body must be a JSON object",
                item.custom_id
            ))
        })?;
        if object.get("stream").and_then(Value::as_bool) == Some(true) {
            return Err(GatewayError::InvalidRequest(format!(
                "batch item `{}` cannot request streaming",
                item.custom_id
            )));
        }
    }
    Ok(())
}

fn request_hash(input: &CreateBatchInput) -> Result<String, GatewayError> {
    let bytes = serde_json::to_vec(&(input.endpoint, &input.model, &input.items))
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
