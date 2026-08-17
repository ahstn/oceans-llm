use std::collections::{BTreeMap, BTreeSet};

use gateway_core::{
    AuthenticatedApiKey, BatchEndpoint, BatchJobRecord, BatchPricingStatus, BatchRepository,
    BatchStatus, BudgetAlertRepository, BudgetRepository, GatewayError, IdentityRepository,
    McpToolInvocationRepository, ModelRepository, ModelRoute, Money4, NewBatchItem, NewBatchJob,
    PricingCatalogRepository, PricingResolution, ProviderRegistry, ProviderRepository,
    ProviderRequestContext, RequestLogRepository, RoutePlanner, StoreHealth,
    is_supported_vertex_google_chat_upstream_model,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    GatewayService,
    service::{
        CacheUsageNormalization, UsageSummary, scaled_cost_for_tokens, usage_summary_from_value,
    },
};

pub const MAX_BATCH_ITEMS: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchPricingPolicy {
    HalfAllTokenRates,
    VertexHalfNonCachedRates,
}

#[derive(Debug, Clone, Copy)]
pub struct BatchPricer {
    rates: Option<BatchRates>,
    policy: BatchPricingPolicy,
}

impl BatchPricer {
    pub fn price_usage(
        &self,
        provider_usage: Option<&Value>,
    ) -> Result<Option<Money4>, GatewayError> {
        let Some(rates) = self.rates else {
            return Ok(None);
        };
        let usage = usage_summary_from_value(provider_usage)?;
        if !usage.has_usage() {
            return Ok(None);
        }
        compute_batch_usage_cost(&usage, rates, self.policy)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BatchRates {
    pub(crate) input: Option<Money4>,
    pub(crate) output: Option<Money4>,
    pub(crate) cache_read: Option<Money4>,
    pub(crate) cache_write: Option<Money4>,
}

impl<S, P> GatewayService<S, P>
where
    S: gateway_core::ApiKeyRepository
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
    pub async fn batch_pricer(
        &self,
        route: &ModelRoute,
        policy: BatchPricingPolicy,
        occurred_at: OffsetDateTime,
    ) -> Result<BatchPricer, GatewayError> {
        let rates = match self.resolve_route_pricing(route, occurred_at).await? {
            PricingResolution::Exact { pricing } => Some(BatchRates {
                input: pricing.input_cost_per_million_tokens,
                output: pricing.output_cost_per_million_tokens,
                cache_read: pricing.cache_read_cost_per_million_tokens,
                cache_write: pricing.cache_write_cost_per_million_tokens,
            }),
            PricingResolution::ConfiguredOverride { pricing } => Some(BatchRates {
                input: Some(pricing.input_cost_per_million_tokens),
                output: Some(pricing.output_cost_per_million_tokens),
                cache_read: pricing.cache_read_cost_per_million_tokens,
                cache_write: pricing.cache_write_cost_per_million_tokens,
            }),
            PricingResolution::Unpriced { .. } => None,
        };
        Ok(BatchPricer { rates, policy })
    }
}

pub(crate) fn compute_batch_usage_cost(
    usage: &UsageSummary,
    rates: BatchRates,
    policy: BatchPricingPolicy,
) -> Result<Option<Money4>, GatewayError> {
    let mut components = Vec::new();
    match &usage.cache_usage {
        CacheUsageNormalization::Valid(_) => {
            components.push((usage.uncached_input_tokens, rates.input, true));
            components.push((
                usage.cache_read_tokens,
                rates.cache_read,
                policy == BatchPricingPolicy::HalfAllTokenRates,
            ));
            components.push((usage.cache_write_tokens, rates.cache_write, true));
        }
        CacheUsageNormalization::Unavailable | CacheUsageNormalization::Invalid(_) => {
            components.push((usage.prompt_tokens, rates.input, true));
        }
        CacheUsageNormalization::Unsupported(_) => return Ok(None),
    }
    components.push((usage.completion_tokens, rates.output, true));

    let mut total = Money4::ZERO;
    for (tokens, rate, discounted) in components {
        let tokens = tokens.unwrap_or_default();
        if tokens == 0 {
            continue;
        }
        let Some(rate) = rate else {
            return Ok(None);
        };
        let mut cost = scaled_cost_for_tokens(tokens, rate)?;
        if discounted {
            cost = Money4::from_scaled((cost.as_scaled_i64() + 1) / 2);
        }
        total = total
            .checked_add(cost)
            .ok_or_else(|| GatewayError::Internal("batch usage cost overflow".to_string()))?;
    }
    Ok(Some(total))
}

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
        .find(|(route, provider)| route_supports_batch(route, provider.as_ref(), input.endpoint))
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

fn route_supports_batch(
    route: &ModelRoute,
    provider: &dyn gateway_core::ProviderClient,
    endpoint: BatchEndpoint,
) -> bool {
    if !provider.batch_capabilities().supports(endpoint) {
        return false;
    }
    let route_supports_endpoint = match endpoint {
        BatchEndpoint::ChatCompletions => route.capabilities.chat_completions,
        BatchEndpoint::Responses => route.capabilities.responses,
        BatchEndpoint::Embeddings => route.capabilities.embeddings,
    };
    route_supports_endpoint
        && (provider.provider_type() != "gcp_vertex"
            || is_supported_vertex_google_chat_upstream_model(&route.upstream_model))
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
