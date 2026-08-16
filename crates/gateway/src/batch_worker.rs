use std::{collections::BTreeSet, sync::Arc, time::Duration};

use gateway_core::{
    AdminApiKeyRepository, AuthenticatedApiKey, BatchPollUpdate, BatchPricingStatus,
    BatchRepository, BatchStatus, ModelRepository, Money4, ProviderBatchRequest,
    ProviderBatchRequestItem, ProviderBatchResult, ProviderError, ProviderRegistry,
};
use gateway_service::BatchPricingPolicy;
use serde_json::json;
use time::OffsetDateTime;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::http::state::AppGatewayService;

const WORKER_INTERVAL: Duration = Duration::from_secs(5);
const LEASE_DURATION: time::Duration = time::Duration::minutes(2);
const POLL_INTERVAL: time::Duration = time::Duration::seconds(30);
const ERROR_RETRY_INTERVAL: time::Duration = time::Duration::minutes(2);

pub fn spawn(service: Arc<AppGatewayService>, providers: ProviderRegistry) {
    let worker_id = format!("{}:{}", std::process::id(), Uuid::new_v4());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(WORKER_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(error) = run_once(&service, &providers, &worker_id).await {
                error!(error = %error, "batch worker iteration failed");
            }
        }
    });
}

async fn run_once(
    service: &Arc<AppGatewayService>,
    providers: &ProviderRegistry,
    worker_id: &str,
) -> Result<(), gateway_core::GatewayError> {
    let now = OffsetDateTime::now_utc();
    let stale = service
        .store()
        .mark_stale_batch_submissions_unknown(now)
        .await?;
    if stale > 0 {
        warn!(count = stale, "marked stale batch submissions as unknown");
    }
    let jobs = service
        .store()
        .claim_batch_jobs(worker_id, now, now + LEASE_DURATION, 8)
        .await?;
    for job in jobs {
        if let Err(error) = process_job(service, providers, worker_id, &job).await {
            warn!(batch_id = %job.batch_id, error = %error, "batch job processing failed");
        }
    }
    Ok(())
}

async fn process_job(
    service: &Arc<AppGatewayService>,
    providers: &ProviderRegistry,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
) -> Result<(), gateway_core::GatewayError> {
    let Some(provider) = providers.get(&job.provider_key) else {
        return fail_claimed_job(
            service,
            worker_id,
            job,
            BatchStatus::Failed,
            &format!("provider `{}` is not registered", job.provider_key),
        )
        .await;
    };
    match job.status {
        BatchStatus::Submitting => submit_job(service, provider.as_ref(), worker_id, job).await,
        BatchStatus::CancelRequested => {
            let Some(provider_batch_id) = job.provider_batch_id.as_deref() else {
                return fail_claimed_job(
                    service,
                    worker_id,
                    job,
                    BatchStatus::Failed,
                    "cancelled batch has no provider batch ID",
                )
                .await;
            };
            match provider.cancel_batch(provider_batch_id).await {
                Ok(state) => apply_state(service, provider.as_ref(), worker_id, job, state).await,
                Err(error) => release_after_provider_error(service, worker_id, job, error).await,
            }
        }
        BatchStatus::Validating
        | BatchStatus::InProgress
        | BatchStatus::Finalizing
        | BatchStatus::Cancelling => {
            let Some(provider_batch_id) = job.provider_batch_id.as_deref() else {
                return fail_claimed_job(
                    service,
                    worker_id,
                    job,
                    BatchStatus::Failed,
                    "active batch has no provider batch ID",
                )
                .await;
            };
            match provider.inspect_batch(provider_batch_id).await {
                Ok(state) => apply_state(service, provider.as_ref(), worker_id, job, state).await,
                Err(error) => release_after_provider_error(service, worker_id, job, error).await,
            }
        }
        _ => Ok(()),
    }
}

async fn submit_job(
    service: &Arc<AppGatewayService>,
    provider: &dyn gateway_core::ProviderClient,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
) -> Result<(), gateway_core::GatewayError> {
    let items = service
        .store()
        .get_batch_items_for_worker(job.batch_id)
        .await?;
    let request = ProviderBatchRequest {
        batch_id: job.batch_id,
        endpoint: job.endpoint,
        upstream_model: job.upstream_model.clone(),
        items: items
            .into_iter()
            .map(|item| ProviderBatchRequestItem {
                custom_id: item.custom_id,
                body: item.request_body,
            })
            .collect(),
        context: job.provider_context.clone(),
    };
    match provider.submit_batch(&request).await {
        Ok(mut state) => {
            if state.status == BatchStatus::Completed {
                state.status = BatchStatus::Finalizing;
                state.completed_at = None;
            }
            service
                .store()
                .mark_batch_submitted(
                    job.batch_id,
                    worker_id,
                    &state,
                    OffsetDateTime::now_utc() + POLL_INTERVAL,
                )
                .await?;
            info!(batch_id = %job.batch_id, provider_batch_id = %state.provider_batch_id, "batch submitted");
            Ok(())
        }
        Err(error) if submission_outcome_is_unknown(&error) => {
            fail_claimed_job(
                service,
                worker_id,
                job,
                BatchStatus::SubmissionUnknown,
                &error.to_string(),
            )
            .await
        }
        Err(error) => {
            fail_claimed_job(
                service,
                worker_id,
                job,
                BatchStatus::Failed,
                &error.to_string(),
            )
            .await
        }
    }
}

async fn apply_state(
    service: &Arc<AppGatewayService>,
    provider: &dyn gateway_core::ProviderClient,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
    mut state: gateway_core::ProviderBatchState,
) -> Result<(), gateway_core::GatewayError> {
    let mut results = Vec::new();
    let mut pricing_status = None;
    if state.status == BatchStatus::Completed {
        results = match provider.batch_results(&state, &job.provider_context).await {
            Ok(results) => results,
            Err(error) => {
                return release_after_provider_error(service, worker_id, job, error).await;
            }
        };
        if let Err(error) = validate_results(service, job.batch_id, &results).await {
            service
                .store()
                .release_batch_lease_after_error(
                    job.batch_id,
                    worker_id,
                    &json!({"message": error.to_string()}),
                    OffsetDateTime::now_utc() + ERROR_RETRY_INTERVAL,
                )
                .await?;
            return Ok(());
        }
        pricing_status = Some(
            price_results(
                service,
                provider.provider_type(),
                job,
                &mut state,
                &mut results,
            )
            .await?,
        );
        record_batch_usage(
            service,
            job,
            &results,
            pricing_status.expect("terminal batch pricing status"),
            state.provider_cost_usd,
            state.completed_at.unwrap_or_else(OffsetDateTime::now_utc),
        )
        .await?;
    }
    let next_poll_at =
        (!state.status.is_terminal()).then(|| OffsetDateTime::now_utc() + POLL_INTERVAL);
    service
        .store()
        .apply_batch_poll_update(
            job.batch_id,
            worker_id,
            &BatchPollUpdate {
                state,
                results,
                next_poll_at,
                pricing_status,
            },
        )
        .await?;
    Ok(())
}

async fn record_batch_usage(
    service: &Arc<AppGatewayService>,
    job: &gateway_core::BatchJobRecord,
    results: &[ProviderBatchResult],
    pricing_status: BatchPricingStatus,
    cost_usd: Option<Money4>,
    occurred_at: OffsetDateTime,
) -> Result<(), gateway_core::GatewayError> {
    let key = service
        .store()
        .get_api_key_by_id(job.api_key_id)
        .await?
        .ok_or_else(|| {
            gateway_core::GatewayError::Internal(format!(
                "batch API key `{}` no longer exists",
                job.api_key_id
            ))
        })?;
    let auth = AuthenticatedApiKey {
        id: key.id,
        public_id: key.public_id,
        name: key.name,
        model_grant_mode: key.model_grant_mode,
        owner_kind: key.owner_kind,
        owner_user_id: key.owner_user_id,
        owner_team_id: key.owner_team_id,
        owner_service_account_id: key.owner_service_account_id,
    };
    service
        .record_batch_usage(&auth, job, results, pricing_status, cost_usd, occurred_at)
        .await
}

async fn validate_results(
    service: &Arc<AppGatewayService>,
    batch_id: Uuid,
    results: &[ProviderBatchResult],
) -> Result<(), gateway_core::GatewayError> {
    let expected = service
        .store()
        .get_batch_items_for_worker(batch_id)
        .await?
        .into_iter()
        .map(|item| item.custom_id)
        .collect::<BTreeSet<_>>();
    let actual = results
        .iter()
        .map(|item| item.custom_id.clone())
        .collect::<BTreeSet<_>>();
    if expected != actual || actual.len() != results.len() {
        return Err(gateway_core::GatewayError::Internal(
            "provider batch results did not contain exactly one result for each custom_id"
                .to_string(),
        ));
    }
    Ok(())
}

async fn price_results(
    service: &Arc<AppGatewayService>,
    provider_type: &str,
    job: &gateway_core::BatchJobRecord,
    state: &mut gateway_core::ProviderBatchState,
    results: &mut [ProviderBatchResult],
) -> Result<BatchPricingStatus, gateway_core::GatewayError> {
    if state.provider_cost_usd.is_some() || results.iter().all(|result| result.cost_usd.is_some()) {
        if state.provider_cost_usd.is_none() {
            state.provider_cost_usd =
                sum_costs(results.iter().filter_map(|result| result.cost_usd))?;
        }
        return Ok(BatchPricingStatus::ProviderReported);
    }
    let route = service
        .store()
        .list_routes_for_model(job.model_id)
        .await?
        .into_iter()
        .find(|route| route.id == job.route_id)
        .ok_or_else(|| {
            gateway_core::GatewayError::Internal(format!(
                "batch route `{}` is no longer available for pricing",
                job.route_id
            ))
        })?;
    let policy = if provider_type == "gcp_vertex" {
        BatchPricingPolicy::VertexHalfNonCachedRates
    } else {
        BatchPricingPolicy::HalfAllTokenRates
    };
    let mut priced = 0_usize;
    for result in results.iter_mut() {
        if result.cost_usd.is_none() && result.error.is_none() {
            result.cost_usd = service
                .price_batch_usage(
                    &route,
                    result.provider_usage.as_ref(),
                    policy,
                    state.completed_at.unwrap_or_else(OffsetDateTime::now_utc),
                )
                .await?;
        }
        if result.cost_usd.is_some() || result.error.is_some() {
            priced += 1;
        }
    }
    state.provider_cost_usd = sum_costs(results.iter().filter_map(|result| result.cost_usd))?;
    Ok(if priced == results.len() {
        BatchPricingStatus::Priced
    } else if priced == 0 {
        BatchPricingStatus::Unpriced
    } else {
        BatchPricingStatus::PartiallyPriced
    })
}

fn sum_costs(
    costs: impl Iterator<Item = Money4>,
) -> Result<Option<Money4>, gateway_core::GatewayError> {
    let mut seen = false;
    let mut total = Money4::ZERO;
    for cost in costs {
        seen = true;
        total = total.checked_add(cost).ok_or_else(|| {
            gateway_core::GatewayError::Internal("batch cost overflow".to_string())
        })?;
    }
    Ok(seen.then_some(total))
}

async fn fail_claimed_job(
    service: &Arc<AppGatewayService>,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
    status: BatchStatus,
    message: &str,
) -> Result<(), gateway_core::GatewayError> {
    service
        .store()
        .mark_batch_submission_failed(
            job.batch_id,
            worker_id,
            status,
            &json!({"message": message}),
            OffsetDateTime::now_utc(),
        )
        .await?;
    Ok(())
}

async fn release_after_provider_error(
    service: &Arc<AppGatewayService>,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
    error: ProviderError,
) -> Result<(), gateway_core::GatewayError> {
    service
        .store()
        .release_batch_lease_after_error(
            job.batch_id,
            worker_id,
            &json!({"message": error.to_string()}),
            OffsetDateTime::now_utc() + ERROR_RETRY_INTERVAL,
        )
        .await?;
    Ok(())
}

fn submission_outcome_is_unknown(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Timeout | ProviderError::Transport(_))
}
