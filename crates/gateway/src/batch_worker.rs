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
use gateway_guardrails::DecisionRecord;
use gateway_service::{BatchPricer, BatchPricingPolicy, BatchUsageInput};
use serde_json::json;
use time::OffsetDateTime;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::http::{
    guardrail_events::record_guardrail_decisions,
    inference_guardrails::{
        batch_guard_context, guard_batch_prompt, guard_model_response, model_route_key,
        redact_prompt_body,
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
    // A worker that died mid-submission may not have stored the redacted
    // requests, and the transition below makes the job terminal.
    for job in service.store().list_stale_batch_submissions(now).await? {
        redact_unsubmitted_batch(state, &job).await?;
    }
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
        if job.status == BatchStatus::Submitting {
            // Never submitted, so `submit_job` has not redacted the stored requests yet.
            redact_unsubmitted_batch(state, job).await?;
        }
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

/// Redacts the stored requests of a batch that ends before submission, such as
/// one cancelled while queued, so its results never return the original secrets.
/// Redaction is idempotent, so callers run it before the terminal transition and a
/// failed attempt can simply be retried.
pub(crate) async fn redact_unsubmitted_batch(
    state: &AppState,
    job: &gateway_core::BatchJobRecord,
) -> Result<(), gateway_core::GatewayError> {
    let items = state
        .service
        .store()
        .get_batch_items_for_worker(job.batch_id)
        .await?;
    let route_key = model_route_key(
        &job.resolved_model_key,
        &job.provider_key,
        &job.upstream_model,
    );
    // Only rewrite the items that held a secret, so a large batch with few
    // findings costs few updates.
    let mut redacted = Vec::new();
    let mut decisions = Vec::new();
    for item in items {
        let mut body = item.request_body;
        let redactions = redact_prompt_body(state, &route_key, &mut body);
        if redactions.is_empty() {
            continue;
        }
        decisions.push((
            format!("batch:{}:{}", job.batch_id, item.custom_id),
            redactions,
        ));
        redacted.push(ProviderBatchRequestItem {
            custom_id: item.custom_id,
            body,
        });
    }
    if redacted.is_empty() {
        return Ok(());
    }
    state
        .service
        .store()
        .replace_batch_item_requests(job.batch_id, &redacted)
        .await?;
    record_in_background(state, decisions);
    Ok(())
}

/// Persists redaction decisions without holding up the batch: decision logging
/// is best effort, and a large batch would otherwise wait on one write per item.
fn record_in_background(state: &AppState, decisions: Vec<(String, Vec<DecisionRecord>)>) {
    if decisions.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        for (request_id, redactions) in decisions {
            record_guardrail_decisions(&state, Some(&request_id), None, &redactions).await;
        }
    });
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
    let mut decisions = Vec::new();
    let mut rejection = None;
    for item in items {
        let mut body = item.request_body;
        let request_id = format!("batch:{}:{}", job.batch_id, item.custom_id);
        if rejection.is_some() {
            // The batch is already rejected; only redact, so stored items keep no secrets.
            let redactions = redact_prompt_body(state, &route_key, &mut body);
            if !redactions.is_empty() {
                decisions.push((request_id, redactions));
            }
        } else if let Err(error) = guard_batch_prompt(
            state,
            &request_id,
            route_key.clone(),
            &mut body,
            &mut decisions,
        )
        .await
        {
            rejection = Some(error.error_code());
        }
        guarded_items.push(ProviderBatchRequestItem {
            custom_id: item.custom_id,
            body,
        });
    }
    // Persist the guarded bodies before any failure return or provider call. Failed,
    // unknown and submitted batches then all return the redacted requests, and a
    // failed write never leaves an untracked provider batch behind.
    service
        .store()
        .replace_batch_item_requests(job.batch_id, &guarded_items)
        .await?;
    record_in_background(state, decisions);
    if let Some(error_code) = rejection {
        return fail_claimed_job(
            service,
            worker_id,
            job,
            BatchStatus::Failed,
            &format!("batch prompt rejected by guardrails: {error_code}"),
        )
        .await;
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
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
    };

    use async_trait::async_trait;
    use gateway_core::{
        BatchAccessScope, BatchEndpoint, BatchJobRecord, BatchPricingStatus, BatchRepository,
        BatchStatus, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, NewBatchItem,
        NewBatchJob, ProviderBatchRequest, ProviderBatchResult, ProviderBatchState,
        ProviderBatchSubmission, ProviderCapabilities, ProviderClient, ProviderError,
        ProviderRequestContext, ProviderStream,
    };
    use gateway_guardrails::{
        FailureDisposition, GuardPhase, GuardrailEngine, ManagedCheckConfig, ManagedCheckKind,
        ManagedEvaluator, ManagedOutcome, ManagedService, PolicyMode, ReasonCode,
        test_utils::StubManagedEvaluator,
    };
    use serde_json::{Value, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{
        prepare_submitted_state, process_job, redact_unsubmitted_batch, run_once, submit_job,
        validate_result_custom_ids,
    };
    use crate::http::{
        state::AppState,
        test_support::{app_state, seed_stream_cancellation_test},
    };

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

    /// Fails every batch submission with a non-retryable error.
    struct RejectingBatchProvider;

    #[async_trait]
    impl ProviderClient for RejectingBatchProvider {
        fn provider_key(&self) -> &str {
            "vertex"
        }

        fn provider_type(&self) -> &str {
            "openai_compat"
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::all_enabled()
        }

        async fn submit_batch(&self, _request: &ProviderBatchRequest) -> ProviderBatchSubmission {
            ProviderBatchSubmission::NotSubmitted(ProviderError::InvalidRequest(
                "rejected".to_string(),
            ))
        }

        async fn chat_completions(
            &self,
            _request: &CoreChatRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            unreachable!("batch submission does not call chat completions")
        }

        async fn chat_completions_stream(
            &self,
            _request: &CoreChatRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            unreachable!("batch submission does not stream")
        }

        async fn embeddings(
            &self,
            _request: &CoreEmbeddingsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            unreachable!("batch submission does not call embeddings")
        }

        async fn responses(
            &self,
            _request: &CoreResponsesRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            unreachable!("batch submission does not call responses")
        }

        async fn responses_stream(
            &self,
            _request: &CoreResponsesRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            unreachable!("batch submission does not stream")
        }
    }

    fn secret_key() -> String {
        // Assembled at runtime so no literal credential lands in the repository.
        format!(
            "sk-ant-api03-{}AA",
            &"aZ3kQ9mB7xR2tW5nL8pJ4vC6yH1dF0gS".repeat(3)[..93]
        )
    }

    async fn redacting_state() -> (tempfile::TempDir, AppState) {
        let (directory, mut state) = app_state().await;
        seed_stream_cancellation_test(&state.store).await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.secret_redaction.enabled = true;
        (directory, state)
    }

    /// Stores a two-item batch whose prompts both carry `key`, then claims it
    /// for submission.
    async fn claimed_batch(state: &AppState, key: &str) -> (BatchJobRecord, Uuid) {
        let (_, api_key_id) = queued_batch(state, key).await;
        let now = OffsetDateTime::now_utc();
        let mut claimed = state
            .store
            .claim_batch_jobs("worker", now, now + time::Duration::minutes(2), 1)
            .await
            .expect("claim batch");
        (claimed.remove(0), api_key_id)
    }

    /// Stores a queued two-item batch whose prompts both carry `key`.
    async fn queued_batch(state: &AppState, key: &str) -> (BatchJobRecord, Uuid) {
        let auth = state
            .service
            .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
            .await
            .expect("authenticate");
        let resolved = state
            .service
            .resolve_request(&auth, "fast")
            .await
            .expect("resolve model");
        let route = &resolved.routes[0];
        let now = OffsetDateTime::now_utc();
        let batch_id = Uuid::new_v4();
        let items = ["row-1", "row-2"]
            .into_iter()
            .map(|custom_id| NewBatchItem {
                batch_item_id: Uuid::new_v4(),
                custom_id: custom_id.to_string(),
                request_body: json!({
                    "model": "fast",
                    "messages": [{"role": "user", "content": format!("deploy with {key}")}],
                }),
            })
            .collect();
        let job = BatchJobRecord {
            batch_id,
            idempotency_key: "redaction-test".to_string(),
            request_hash: "redaction-test".to_string(),
            api_key_id: auth.id,
            user_id: auth.owner_user_id,
            team_id: auth.owner_team_id,
            service_account_id: auth.owner_service_account_id,
            model_id: resolved.selection.execution_model.id,
            model_key: "fast".to_string(),
            resolved_model_key: resolved.selection.execution_model.model_key.clone(),
            route_id: route.id,
            provider_key: route.provider_key.clone(),
            upstream_model: route.upstream_model.clone(),
            endpoint: BatchEndpoint::ChatCompletions,
            status: BatchStatus::Queued,
            provider_batch_id: None,
            request_count: 2,
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
            provider_context: ProviderRequestContext {
                request_id: batch_id.to_string(),
                model_key: "fast".to_string(),
                provider_key: route.provider_key.clone(),
                upstream_model: route.upstream_model.clone(),
                owner_user_id: auth.owner_user_id,
                expected_provider_credential_id: None,
                extra_headers: Default::default(),
                extra_body: Default::default(),
                request_headers: BTreeMap::new(),
                compatibility: route.compatibility.clone(),
            },
            pricing_snapshot: None,
        };
        state
            .store
            .insert_batch(&NewBatchJob {
                job: job.clone(),
                items,
            })
            .await
            .expect("insert batch");
        (job, auth.id)
    }

    async fn assert_failed_with_redacted_items(
        state: &AppState,
        batch_id: Uuid,
        api_key_id: Uuid,
        key: &str,
    ) {
        assert_redacted_items(state, batch_id, api_key_id, key, BatchStatus::Failed).await;
    }

    async fn assert_redacted_items(
        state: &AppState,
        batch_id: Uuid,
        api_key_id: Uuid,
        key: &str,
        status: BatchStatus,
    ) {
        let batch = state
            .store
            .get_batch(batch_id, BatchAccessScope::ApiKey(api_key_id))
            .await
            .expect("batch");
        assert_eq!(batch.status, status);
        let items = state
            .store
            .get_batch_items_for_worker(batch_id)
            .await
            .expect("batch items");
        assert_eq!(items.len(), 2);
        for item in items {
            assert_eq!(
                item.request_body["messages"][0]["content"],
                "deploy with [REDACTED:anthropic-api-key]",
                "{} kept its secret",
                item.custom_id
            );
            assert!(!item.request_body.to_string().contains(key));
        }
    }

    #[tokio::test]
    async fn batch_cancelled_while_queued_stores_redacted_requests() {
        let (_directory, state) = redacting_state().await;
        let key = secret_key();
        let (job, api_key_id) = queued_batch(&state, &key).await;

        // The cancel endpoint redacts first, so a failed redaction leaves the batch
        // queued and the cancellation can be retried.
        redact_unsubmitted_batch(&state, &job)
            .await
            .expect("redact batch");
        state
            .store
            .request_batch_cancel(
                job.batch_id,
                BatchAccessScope::ApiKey(api_key_id),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("cancel batch");

        assert_redacted_items(
            &state,
            job.batch_id,
            api_key_id,
            &key,
            BatchStatus::Cancelled,
        )
        .await;
    }

    #[tokio::test]
    async fn batch_failed_for_a_missing_provider_stores_redacted_requests() {
        let (_directory, mut state) = redacting_state().await;
        let key = secret_key();
        let (job, api_key_id) = claimed_batch(&state, &key).await;
        state.providers = gateway_core::ProviderRegistry::new();

        process_job(&state, "worker", &job)
            .await
            .expect("missing provider is recorded");

        assert_failed_with_redacted_items(&state, job.batch_id, api_key_id, &key).await;
    }

    #[tokio::test]
    async fn stale_submission_stores_redacted_requests() {
        let (_directory, state) = redacting_state().await;
        let key = secret_key();
        let (job, api_key_id) = queued_batch(&state, &key).await;
        // A worker claimed the job and died before its lease ran out.
        let now = OffsetDateTime::now_utc();
        state
            .store
            .claim_batch_jobs("dead-worker", now, now - time::Duration::seconds(1), 1)
            .await
            .expect("claim batch");

        run_once(&state, "worker").await.expect("worker iteration");

        assert_redacted_items(
            &state,
            job.batch_id,
            api_key_id,
            &key,
            BatchStatus::SubmissionUnknown,
        )
        .await;
    }

    #[tokio::test]
    async fn failed_submission_stores_redacted_requests() {
        let (_directory, state) = redacting_state().await;
        let key = secret_key();
        let (job, api_key_id) = claimed_batch(&state, &key).await;

        submit_job(&state, &RejectingBatchProvider, "worker", &job)
            .await
            .expect("submission failure is recorded");

        assert_failed_with_redacted_items(&state, job.batch_id, api_key_id, &key).await;
    }

    #[tokio::test]
    async fn guardrail_rejection_stores_redacted_requests_for_every_item() {
        let (_directory, mut state) = redacting_state().await;
        let config = Arc::make_mut(&mut state.guardrail_config);
        config.default.mode = PolicyMode::Deny;
        config.default.managed_checks = vec!["deny".into()];
        config.managed_checks.insert(
            "deny".into(),
            ManagedCheckConfig {
                kind: ManagedCheckKind::GoogleModelArmor,
                phases: BTreeSet::from([GuardPhase::Prompt]),
                timeout_ms: 1_000,
                failure_disposition: FailureDisposition::FailClosed,
                max_content_bytes: 4_096,
                bedrock: None,
                model_armor: None,
            },
        );
        // The first item is rejected, so the second is never evaluated.
        state.guardrail_engine = Arc::new(GuardrailEngine::new(
            Vec::new(),
            BTreeMap::from([(
                "deny".into(),
                Arc::new(StubManagedEvaluator::new(
                    "deny",
                    ManagedService::GoogleModelArmor,
                    [Ok(ManagedOutcome::Intervention {
                        reason_code: ReasonCode::new("test.denied").unwrap(),
                        metadata: Default::default(),
                    })],
                )) as Arc<dyn ManagedEvaluator>,
            )]),
        ));
        let key = secret_key();
        let (job, api_key_id) = claimed_batch(&state, &key).await;

        submit_job(&state, &RejectingBatchProvider, "worker", &job)
            .await
            .expect("guardrail rejection is recorded");

        assert_failed_with_redacted_items(&state, job.batch_id, api_key_id, &key).await;
        let batch = state
            .store
            .get_batch(job.batch_id, BatchAccessScope::ApiKey(api_key_id))
            .await
            .expect("batch");
        assert!(
            batch.error.unwrap()["message"]
                .as_str()
                .unwrap()
                .starts_with("batch prompt rejected by guardrails")
        );
    }
}
