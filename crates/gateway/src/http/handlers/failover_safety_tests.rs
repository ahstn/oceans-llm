use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use gateway_guardrails::{
    FailureDisposition, GuardPhase, GuardrailEngine, ManagedCheckConfig, ManagedCheckKind,
    ManagedOutcome, ManagedService, PolicyMode, PolicyOverride, ReasonCode,
    test_utils::StubManagedEvaluator,
};

use super::*;

fn deny_phase(harness: &mut FailoverHarness, phase: GuardPhase, route: Option<&str>) {
    let config = Arc::make_mut(&mut harness.state.guardrail_config);
    if let Some(route) = route {
        config.model_routes.insert(
            route.to_string(),
            PolicyOverride {
                enabled: Some(true),
                mode: Some(PolicyMode::Deny),
                managed_checks: Some(vec!["deny".into()]),
                ..Default::default()
            },
        );
    } else {
        config.default.enabled = true;
        config.default.mode = PolicyMode::Deny;
        config.default.managed_checks = vec!["deny".into()];
    }
    config.managed_checks.insert(
        "deny".into(),
        ManagedCheckConfig {
            kind: ManagedCheckKind::GoogleModelArmor,
            phases: BTreeSet::from([phase]),
            timeout_ms: 1_000,
            failure_disposition: FailureDisposition::FailClosed,
            max_content_bytes: 4_096,
            bedrock: None,
            model_armor: None,
        },
    );
    harness.state.guardrail_engine = Arc::new(GuardrailEngine::new(
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
            )) as Arc<dyn gateway_guardrails::ManagedEvaluator>,
        )]),
    ));
}

#[tokio::test]
async fn copilot_quota_moves_directly_to_another_route() {
    let harness =
        FailoverHarness::with_provider_type([Outcome::Quota, Outcome::Success], "github_copilot")
            .await;
    consume(
        harness
            .call(Endpoint::Chat, "quota", false, None)
            .await
            .unwrap_or_else(|error| panic!("request failed: {}", error.0)),
    )
    .await;
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].provider, calls[1].provider);
    let detail = harness.detail("quota").await;
    assert_attempts(
        &detail,
        &[
            RequestAttemptStatus::ProviderError,
            RequestAttemptStatus::Success,
        ],
        true,
    );
    assert_eq!(detail.attempts[0].status_code, Some(402));
    harness.assert_usage("quota", calls[1].provider).await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn destination_prompt_denial_preserves_only_the_executed_attempt() {
    let mut harness =
        FailoverHarness::with_provider_type([Outcome::Success, Outcome::Quota], "github_copilot")
            .await;
    // Round-robin sends the next request to the other route, then falls back here.
    consume(
        harness
            .call(Endpoint::Chat, "warm-route", false, None)
            .await
            .unwrap_or_else(|error| panic!("request failed: {}", error.0)),
    )
    .await;
    let destination = harness.calls.lock().unwrap()[0].clone();
    deny_phase(
        &mut harness,
        GuardPhase::Prompt,
        Some(&format!(
            "fast/{}/{}",
            destination.provider, destination.upstream_model,
        )),
    );
    let error = harness
        .call(Endpoint::Chat, "prompt-denied", false, None)
        .await
        .unwrap_err();
    assert_eq!(error.0.error_code(), "guardrail_policy_denied");
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[1].provider, destination.provider);
    let detail = harness.detail("prompt-denied").await;
    assert_attempts(&detail, &[RequestAttemptStatus::ProviderError], false);
    assert_eq!(detail.log.status_code, Some(403));
    assert_eq!(detail.log.provider_key, destination.provider);
    assert!(harness.ledgers("prompt-denied").await.is_empty());
    assert_eq!(harness.state.metrics.test_snapshot().requests, 2);
}

#[tokio::test]
async fn response_guard_denial_keeps_incurred_usage_and_never_retries() {
    let mut harness = FailoverHarness::new([Outcome::Success]).await;
    deny_phase(&mut harness, GuardPhase::ModelResponse, None);
    let error = harness
        .call(Endpoint::Chat, "response-denied", false, None)
        .await
        .unwrap_err();
    assert_eq!(error.0.error_code(), "guardrail_policy_denied");
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let detail = harness.detail("response-denied").await;
    assert_attempts(&detail, &[RequestAttemptStatus::Success], false);
    assert!(!detail.attempts[0].retryable);
    assert_eq!(detail.log.status_code, Some(403));
    harness
        .assert_usage("response-denied", calls[0].provider)
        .await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn destination_budget_recheck_stops_before_another_provider_call() {
    let harness =
        FailoverHarness::with_provider_type([Outcome::QuotaExhaustsBudget], "github_copilot").await;
    let error = harness
        .call(Endpoint::Chat, "budget-changed", false, None)
        .await
        .unwrap_err();
    assert_eq!(error.0.error_code(), "budget_exceeded");
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let detail = harness.detail("budget-changed").await;
    assert_attempts(&detail, &[RequestAttemptStatus::ProviderError], false);
    assert_eq!(detail.log.status_code, Some(429));
    assert_ne!(detail.log.provider_key, calls[0].provider);
    assert!(harness.ledgers("budget-changed").await.is_empty());
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn stream_start_retry_keeps_history_through_successful_completion() {
    for endpoint in [Endpoint::Chat, Endpoint::Messages, Endpoint::Responses] {
        let harness = FailoverHarness::new([Outcome::Transient, Outcome::Success]).await;
        let response = harness
            .call(endpoint, "stream-retry", true, None)
            .await
            .unwrap_or_else(|error| panic!("request failed: {}", error.0));
        assert_eq!(response.status(), 200);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let terminal = match endpoint {
            Endpoint::Chat => "[DONE]",
            Endpoint::Messages => "message_stop",
            Endpoint::Responses => "response.completed",
            _ => unreachable!(),
        };
        assert!(String::from_utf8_lossy(&body).contains(terminal));
        let calls = harness.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].provider, calls[1].provider);
        let detail = harness.detail("stream-retry").await;
        assert_attempts(
            &detail,
            &[
                RequestAttemptStatus::StreamStartError,
                RequestAttemptStatus::Success,
            ],
            true,
        );
        assert!(detail.attempts.iter().all(|attempt| attempt.stream));
        harness
            .assert_usage("stream-retry", calls[1].provider)
            .await;
        assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
    }
}

#[tokio::test]
async fn total_attempt_limit_stops_retries_across_the_pool() {
    // Four attempts stop during the second route's retries, before either the
    // two-route pool or the eight queued transient outcomes are exhausted.
    let harness =
        FailoverHarness::with_max_attempts([Outcome::Transient; 8], "openai_compat", 4).await;
    let error = harness
        .call(Endpoint::Chat, "attempt-limit", false, None)
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 503);
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 4);
    assert!(
        calls[..3]
            .iter()
            .all(|call| call.provider == calls[0].provider)
    );
    assert!(
        calls[3..]
            .iter()
            .all(|call| call.provider == calls[3].provider)
    );
    assert_ne!(calls[0].provider, calls[3].provider);
    let detail = harness.detail("attempt-limit").await;
    assert_attempts(&detail, &[RequestAttemptStatus::ProviderError; 4], false);
    assert!(harness.ledgers("attempt-limit").await.is_empty());
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn retry_after_above_backoff_limit_falls_back_without_retrying_the_route() {
    let harness = FailoverHarness::new([Outcome::LongRetryAfter, Outcome::Success]).await;
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        harness.call(Endpoint::Chat, "long-retry-after", false, None),
    )
    .await
    .expect("long retry-after must not block the request")
    .unwrap_or_else(|error| panic!("request failed: {}", error.0));
    consume(response).await;
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].provider, calls[1].provider);
    let detail = harness.detail("long-retry-after").await;
    assert_attempts(
        &detail,
        &[
            RequestAttemptStatus::ProviderError,
            RequestAttemptStatus::Success,
        ],
        true,
    );
    harness
        .assert_usage("long-retry-after", calls[1].provider)
        .await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn cancellation_after_a_retry_keeps_attempt_history_and_observed_usage() {
    let harness = FailoverHarness::new([Outcome::Transient, Outcome::PendingStream]).await;
    let response = harness
        .call(Endpoint::Responses, "cancel-retry", true, None)
        .await
        .unwrap_or_else(|error| panic!("request failed: {}", error.0));
    let mut body = response.into_body().into_data_stream();
    assert!(body.next().await.unwrap().is_ok());
    drop(body);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let page = harness
                .state
                .service
                .list_request_logs(&RequestLogQuery {
                    request_id: Some("cancel-retry".to_string()),
                    page: 1,
                    page_size: 10,
                    ..Default::default()
                })
                .await
                .unwrap();
            if page.total == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled stream log must be persisted");
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].provider, calls[1].provider);
    let detail = harness.detail("cancel-retry").await;
    assert_attempts(
        &detail,
        &[
            RequestAttemptStatus::StreamStartError,
            RequestAttemptStatus::StreamError,
        ],
        false,
    );
    assert_eq!(detail.log.status_code, Some(499));
    assert_eq!(
        detail.attempts[1].error_code.as_deref(),
        Some("client_cancelled")
    );
    harness
        .assert_usage("cancel-retry", calls[1].provider)
        .await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn fully_cooled_pool_records_a_request_without_provider_attempts() {
    let harness =
        FailoverHarness::with_provider_type([Outcome::Quota, Outcome::Quota], "github_copilot")
            .await;
    assert!(
        harness
            .call(Endpoint::Responses, "cool-routes", false, None)
            .await
            .is_err()
    );
    let error = harness
        .call(Endpoint::Responses, "all-cooled", false, None)
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 503);
    assert_eq!(harness.calls.lock().unwrap().len(), 2);
    let detail = harness.detail("all-cooled").await;
    assert!(detail.attempts.is_empty());
    assert_eq!(detail.log.status_code, Some(503));
    assert!(harness.ledgers("all-cooled").await.is_empty());
    assert_eq!(harness.state.metrics.test_snapshot().requests, 2);
}
