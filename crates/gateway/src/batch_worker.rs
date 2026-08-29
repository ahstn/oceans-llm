use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use futures_util::{StreamExt, stream};
use gateway_core::{
    AdminApiKeyRepository, AuthenticatedApiKey, BatchPollUpdate, BatchPricingStatus,
    BatchRepository, BatchStatus, ModelRepository, Money4, ProviderBatchRequest,
    ProviderBatchRequestItem, ProviderBatchResult, ProviderBatchSubmission, ProviderError,
};
use gateway_service::{BatchPricer, BatchPricingPolicy, BatchUsageInput};
use serde_json::json;
use time::OffsetDateTime;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::http::{
    inference_guardrails::{
        batch_guard_context, guard_model_response, guard_prompt, model_route_key,
    },
    state::{AppGatewayService, AppState},
};

const WORKER_INTERVAL: Duration = Duration::from_secs(5);
const LEASE_DURATION: time::Duration = time::Duration::minutes(2);
const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(30);
const MAX_IN_FLIGHT_JOBS: usize = 8;
const POLL_INTERVAL: time::Duration = time::Duration::seconds(30);
const ERROR_RETRY_INTERVAL: time::Duration = time::Duration::minutes(2);

pub fn spawn(state: AppState) {
    let worker_id = format!("{}:{}", std::process::id(), Uuid::new_v4());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(WORKER_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(error) = run_once(&state, &worker_id).await {
                error!(error = %error, "batch worker iteration failed");
            }
        }
    });
}

async fn run_once(state: &AppState, worker_id: &str) -> Result<(), gateway_core::GatewayError> {
    let service = &state.service;
    let now = OffsetDateTime::now_utc();
    let stale = service
        .store()
        .mark_stale_batch_submissions_unknown(now)
        .await?;
    if stale > 0 {
        warn!(count = stale, "marked stale batch submissions as unknown");
    }
    let lease_owner = format!("{worker_id}:{}", Uuid::new_v4());
    let jobs = service
        .store()
        .claim_batch_jobs(
            &lease_owner,
            now,
            now + LEASE_DURATION,
            MAX_IN_FLIGHT_JOBS as u32,
        )
        .await?;
    let results = stream::iter(jobs.into_iter().map(|job| {
        let state = state.clone();
        let lease_owner = lease_owner.clone();
        async move {
            (
                job.batch_id,
                process_job_with_lease(&state, &lease_owner, &job).await,
            )
        }
    }))
    .buffer_unordered(MAX_IN_FLIGHT_JOBS)
    .collect::<Vec<_>>()
    .await;
    for (batch_id, result) in results {
        if let Err(error) = result {
            warn!(batch_id = %batch_id, error = %error, "batch job processing failed");
        }
    }
    Ok(())
}

async fn process_job_with_lease(
    state: &AppState,
    lease_owner: &str,
    job: &gateway_core::BatchJobRecord,
) -> Result<(), gateway_core::GatewayError> {
    let service = &state.service;
    let work = process_job(state, lease_owner, job);
    tokio::pin!(work);
    let mut renewals = tokio::time::interval(LEASE_RENEW_INTERVAL);
    renewals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    renewals.tick().await;
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = renewals.tick() => {
                let now = OffsetDateTime::now_utc();
                if let Err(error) = service
                    .store()
                    .renew_batch_lease(job.batch_id, lease_owner, now, now + LEASE_DURATION)
                    .await
                {
                    warn!(batch_id = %job.batch_id, error = %error, "batch lease renewal failed; waiting for the in-flight provider operation");
                    return work.await;
                }
            }
        }
    }
}

async fn process_job(
    state: &AppState,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
) -> Result<(), gateway_core::GatewayError> {
    let service = &state.service;
    let Some(provider) = state.providers.get(&job.provider_key) else {
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
        BatchStatus::Submitting => submit_job(state, provider.as_ref(), worker_id, job).await,
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
            match provider
                .cancel_batch(provider_batch_id, &job.provider_context)
                .await
            {
                Ok(state_update) => {
                    apply_state(state, provider.as_ref(), worker_id, job, state_update).await
                }
                Err(error) if error.is_retryable() => {
                    handle_provider_error(service, worker_id, job, error).await
                }
                Err(_) => {
                    match provider
                        .inspect_batch(provider_batch_id, &job.provider_context)
                        .await
                    {
                        Ok(state_update) => {
                            apply_state(state, provider.as_ref(), worker_id, job, state_update)
                                .await
                        }
                        Err(inspect_error) => {
                            handle_provider_error(service, worker_id, job, inspect_error).await
                        }
                    }
                }
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
            match provider
                .inspect_batch(provider_batch_id, &job.provider_context)
                .await
            {
                Ok(state_update) => {
                    apply_state(state, provider.as_ref(), worker_id, job, state_update).await
                }
                Err(error) => handle_provider_error(service, worker_id, job, error).await,
            }
        }
        _ => Ok(()),
    }
}

async fn submit_job(
    state: &AppState,
    provider: &dyn gateway_core::ProviderClient,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
) -> Result<(), gateway_core::GatewayError> {
    let service = &state.service;
    let items = service
        .store()
        .get_batch_items_for_worker(job.batch_id)
        .await?;
    let route_key = model_route_key(
        &job.resolved_model_key,
        &job.provider_key,
        &job.upstream_model,
    );
    let mut guarded_items = Vec::with_capacity(items.len());
    for item in items {
        let mut body = item.request_body;
        let request_id = format!("batch:{}:{}", job.batch_id, item.custom_id);
        if let Err(error) = guard_prompt(state, &request_id, route_key.clone(), &mut body).await {
            return fail_claimed_job(
                service,
                worker_id,
                job,
                BatchStatus::Failed,
                &format!(
                    "batch prompt rejected by guardrails: {}",
                    error.error_code()
                ),
            )
            .await;
        }
        guarded_items.push(ProviderBatchRequestItem {
            custom_id: item.custom_id,
            body,
        });
    }
    let request = ProviderBatchRequest {
        batch_id: job.batch_id,
        endpoint: job.endpoint,
        upstream_model: job.upstream_model.clone(),
        items: guarded_items,
        context: job.provider_context.clone(),
    };
    match provider.submit_batch(&request).await {
        ProviderBatchSubmission::Submitted(state) => {
            service
                .store()
                .replace_batch_item_requests(job.batch_id, &request.items)
                .await?;
            let state = prepare_submitted_state(state);
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
        ProviderBatchSubmission::SubmissionUnknown(error) => {
            fail_claimed_job(
                service,
                worker_id,
                job,
                BatchStatus::SubmissionUnknown,
                &error.to_string(),
            )
            .await
        }
        ProviderBatchSubmission::NotSubmitted(error) => {
            if error.is_retryable() {
                handle_provider_error(service, worker_id, job, error).await
            } else {
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
}

fn prepare_submitted_state(
    mut state: gateway_core::ProviderBatchState,
) -> gateway_core::ProviderBatchState {
    if state.status.is_terminal() {
        state.status = BatchStatus::Finalizing;
        state.completed_at = None;
    }
    state
}

async fn apply_state(
    app_state: &AppState,
    provider: &dyn gateway_core::ProviderClient,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
    mut state: gateway_core::ProviderBatchState,
) -> Result<(), gateway_core::GatewayError> {
    let service = &app_state.service;
    let mut results = Vec::new();
    let mut pricing_status = None;
    let complete = state.status == BatchStatus::Completed;
    let partial_terminal = matches!(
        state.status,
        BatchStatus::Failed | BatchStatus::Expired | BatchStatus::Cancelled
    );
    let processed_count = partial_terminal
        .then(|| processed_result_count(&state))
        .transpose()?;
    if complete || processed_count.is_some_and(|count| count > 0) {
        results = match provider.batch_results(&state, &job.provider_context).await {
            Ok(results) => results,
            Err(error) => {
                return handle_provider_error(service, worker_id, job, error).await;
            }
        };
        if let Err(error) = validate_results(service, job.batch_id, &results, processed_count).await
        {
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
    }
    if complete || !results.is_empty() || state.provider_cost_usd.is_some() {
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
        guard_batch_results(app_state, job, &mut results).await?;
        record_batch_usage(
            service,
            job,
            &results,
            state.provider_usage.as_ref(),
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

async fn guard_batch_results(
    state: &AppState,
    job: &gateway_core::BatchJobRecord,
    results: &mut [ProviderBatchResult],
) -> Result<(), gateway_core::GatewayError> {
    let requests = state
        .service
        .store()
        .get_batch_items_for_worker(job.batch_id)
        .await?
        .into_iter()
        .map(|item| (item.custom_id, item.request_body))
        .collect::<BTreeMap<_, _>>();
    let route_key = model_route_key(
        &job.resolved_model_key,
        &job.provider_key,
        &job.upstream_model,
    );
    for result in results {
        let Some(request) = requests.get(&result.custom_id) else {
            continue;
        };
        let guarded_payload = if let Some(response) = result.response_body.as_mut() {
            response
        } else if let Some(error) = result.error.as_mut() {
            error
        } else {
            continue;
        };
        let request_id = format!("batch:{}:{}", job.batch_id, result.custom_id);
        let context = batch_guard_context(state, request_id, route_key.clone(), request);
        if let Err(error) = guard_model_response(state, &context, guarded_payload).await {
            result.response_body = None;
            result.error = Some(json!({
                "message": "Batch result was rejected by guardrails",
                "type": error.error_type(),
                "code": error.error_code(),
            }));
        }
    }
    Ok(())
}

async fn record_batch_usage(
    service: &Arc<AppGatewayService>,
    job: &gateway_core::BatchJobRecord,
    results: &[ProviderBatchResult],
    provider_usage: Option<&serde_json::Value>,
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
        .record_batch_usage(
            &auth,
            job,
            BatchUsageInput {
                results,
                provider_usage,
                pricing_status,
                cost_usd,
                occurred_at,
            },
        )
        .await
}

async fn validate_results(
    service: &Arc<AppGatewayService>,
    batch_id: Uuid,
    results: &[ProviderBatchResult],
    expected_result_count: Option<usize>,
) -> Result<(), gateway_core::GatewayError> {
    let expected = service
        .store()
        .get_batch_items_for_worker(batch_id)
        .await?
        .into_iter()
        .map(|item| item.custom_id)
        .collect::<BTreeSet<_>>();
    validate_result_custom_ids(&expected, results, expected_result_count)
}

fn processed_result_count(
    state: &gateway_core::ProviderBatchState,
) -> Result<usize, gateway_core::GatewayError> {
    let processed = state
        .completed_count
        .checked_add(state.failed_count)
        .ok_or_else(|| {
            gateway_core::GatewayError::Internal(
                "provider batch processed count overflow".to_string(),
            )
        })?;
    usize::try_from(processed).map_err(|_| {
        gateway_core::GatewayError::Internal(format!(
            "provider batch returned invalid processed count `{processed}`"
        ))
    })
}

fn validate_result_custom_ids(
    expected: &BTreeSet<String>,
    results: &[ProviderBatchResult],
    expected_result_count: Option<usize>,
) -> Result<(), gateway_core::GatewayError> {
    let actual = results
        .iter()
        .map(|item| item.custom_id.clone())
        .collect::<BTreeSet<_>>();
    if actual.len() != results.len() || !actual.is_subset(expected) {
        return Err(gateway_core::GatewayError::Internal(
            "provider batch results contained a duplicate or unknown custom_id".to_string(),
        ));
    }
    match expected_result_count {
        Some(count) if results.len() != count => {
            return Err(gateway_core::GatewayError::Internal(format!(
                "provider batch returned {} results for {count} processed requests",
                results.len()
            )));
        }
        None if actual != *expected => {
            return Err(gateway_core::GatewayError::Internal(
                "provider batch results did not contain exactly one result for each custom_id"
                    .to_string(),
            ));
        }
        _ => {}
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
    let pricer = if let Some(snapshot) = job.pricing_snapshot {
        BatchPricer::from_snapshot(snapshot)
    } else {
        let route = service
            .store()
            .list_routes_for_model(job.model_id)
            .await?
            .into_iter()
            .find(|route| route.id == job.route_id)
            .ok_or_else(|| {
                gateway_core::GatewayError::Internal(format!(
                    "batch route `{}` is no longer available for legacy pricing",
                    job.route_id
                ))
            })?;
        let policy = if provider_type == "gcp_vertex" {
            BatchPricingPolicy::VertexHalfNonCachedRates
        } else {
            BatchPricingPolicy::HalfAllTokenRates
        };
        service
            .batch_pricer(
                &route,
                policy,
                state.completed_at.unwrap_or_else(OffsetDateTime::now_utc),
            )
            .await?
    };
    let mut successful = 0_usize;
    let mut priced = 0_usize;
    let mut locally_priced = vec![false; results.len()];
    for (index, result) in results.iter_mut().enumerate() {
        if result.error.is_none() {
            successful += 1;
            if result.cost_usd.is_none() {
                result.cost_usd = pricer.price_usage(result.provider_usage.as_ref())?;
                locally_priced[index] = result.cost_usd.is_some();
            }
            if result.cost_usd.is_some() {
                priced += 1;
            }
        }
    }
    let reported_cost = sum_costs(results.iter().zip(&locally_priced).filter_map(
        |(result, locally_priced)| {
            if *locally_priced {
                None
            } else {
                result.cost_usd
            }
        },
    ))?;
    let local_cost = pricer.price_usages(results.iter().zip(&locally_priced).filter_map(
        |(result, locally_priced)| locally_priced.then_some(result.provider_usage.as_ref()),
    ))?;
    state.provider_cost_usd = sum_costs([reported_cost, local_cost].into_iter().flatten())?;
    Ok(if successful > 0 && priced == successful {
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

async fn handle_provider_error(
    service: &Arc<AppGatewayService>,
    worker_id: &str,
    job: &gateway_core::BatchJobRecord,
    error: ProviderError,
) -> Result<(), gateway_core::GatewayError> {
    if !error.is_retryable() {
        return fail_claimed_job(
            service,
            worker_id,
            job,
            BatchStatus::Failed,
            &error.to_string(),
        )
        .await;
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use gateway_core::{BatchStatus, ProviderBatchResult, ProviderBatchState};
    use time::OffsetDateTime;

    use super::{prepare_submitted_state, validate_result_custom_ids};

    fn result(custom_id: &str) -> ProviderBatchResult {
        ProviderBatchResult {
            custom_id: custom_id.to_string(),
            response_body: None,
            error: None,
            provider_request_id: None,
            provider_usage: None,
            completed_at: None,
            cost_usd: None,
        }
    }

    fn expected() -> BTreeSet<String> {
        ["one", "two", "three"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn terminal_partial_results_accept_the_exact_processed_subset() {
        let results = vec![result("one"), result("three")];

        validate_result_custom_ids(&expected(), &results, Some(2))
            .expect("processed result subset");
    }

    #[test]
    fn terminal_partial_results_reject_an_incorrect_count() {
        let results = vec![result("one")];

        assert!(validate_result_custom_ids(&expected(), &results, Some(2)).is_err());
    }

    #[test]
    fn terminal_partial_results_reject_duplicate_or_unknown_ids() {
        let duplicate = vec![result("one"), result("one")];
        let unknown = vec![result("one"), result("unknown")];

        assert!(validate_result_custom_ids(&expected(), &duplicate, Some(2)).is_err());
        assert!(validate_result_custom_ids(&expected(), &unknown, Some(2)).is_err());
    }

    #[test]
    fn immediate_terminal_submission_is_finalized_before_storage() {
        for status in [
            BatchStatus::Completed,
            BatchStatus::Failed,
            BatchStatus::Expired,
            BatchStatus::Cancelled,
        ] {
            let state = prepare_submitted_state(ProviderBatchState {
                provider_batch_id: "provider-batch".to_string(),
                status,
                request_count: 1,
                completed_count: 0,
                failed_count: 1,
                provider_usage: None,
                provider_cost_usd: None,
                error: None,
                submitted_at: None,
                completed_at: Some(OffsetDateTime::now_utc()),
            });
            assert_eq!(state.status, BatchStatus::Finalizing);
            assert!(state.completed_at.is_none());
        }
    }
}
