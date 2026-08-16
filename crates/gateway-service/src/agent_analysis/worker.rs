use super::*;

pub async fn enqueue_analysis<S>(
    store: &S,
    agent_session_id: Uuid,
    reason: &str,
    dedupe_key: &str,
    now: OffsetDateTime,
) -> Result<bool, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let desired_versions = desired_versions();
    enqueue_analysis_with_versions(
        store,
        agent_session_id,
        reason,
        dedupe_key,
        now,
        &desired_versions,
    )
    .await
}

pub async fn enqueue_analysis_with_versions<S>(
    store: &S,
    agent_session_id: Uuid,
    reason: &str,
    dedupe_key: &str,
    now: OffsetDateTime,
    desired_versions: &AgentAnalysisDesiredVersions,
) -> Result<bool, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    store
        .enqueue_agent_analysis(&AgentAnalysisQueueRecord {
            queue_item_id: stable_uuid(
                QUEUE_ID_NAMESPACE,
                &json!({
                    "session": agent_session_id,
                    "reason": reason,
                    "versions": desired_versions,
                    "dedupe_key": dedupe_key,
                })
                .to_string(),
            ),
            agent_session_id,
            reason: reason.to_string(),
            desired_versions: desired_versions.clone(),
            status: AgentAnalysisQueueStatus::Pending,
            lease_owner: None,
            lease_expires_at: None,
            attempts: 0,
            max_attempts: 5,
            last_error: None,
            available_at: now,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .await
        .map_err(Into::into)
}

pub async fn finalize_idle_sessions<S>(
    store: &S,
    now: OffsetDateTime,
    desired_versions: &AgentAnalysisDesiredVersions,
) -> Result<u64, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let cutoff = now - SESSION_IDLE_GAP;
    let mut finalized = 0_u64;
    for _ in 0..MAX_IDLE_FINALIZATION_PAGES {
        let page = store
            .list_agent_sessions(&AgentSessionListQuery {
                page: 1,
                page_size: gateway_core::MAX_AGENT_SESSION_PAGE_SIZE,
                lifecycle: Some(SessionLifecycleState::Open),
                started_before: Some(cutoff),
                input_watermark_before: Some(cutoff),
                ..Default::default()
            })
            .await?;
        let candidates: Vec<_> = page.items.into_iter().map(|trace| trace.session).collect();
        if candidates.is_empty() {
            break;
        }
        let finalized_before_page = finalized;
        for mut session in candidates {
            let last_activity_at = session.input_watermark_at;
            session.lifecycle = SessionLifecycleState::Finalized;
            session.ended_at = Some(last_activity_at);
            session.input_watermark_at = now;
            session.finalized_reason = Some("idle_gap".to_string());
            session.updated_at = now;
            if !store
                .finalize_agent_session_if_unchanged(&session, last_activity_at)
                .await?
            {
                continue;
            }
            store
                .mark_agent_session_analyses_stale(session.agent_session_id, None)
                .await?;
            enqueue_analysis_with_versions(
                store,
                session.agent_session_id,
                "session_finalized",
                &last_activity_at.unix_timestamp_nanos().to_string(),
                now,
                desired_versions,
            )
            .await?;
            finalized = finalized.saturating_add(1);
        }
        if page.total <= u64::from(gateway_core::MAX_AGENT_SESSION_PAGE_SIZE) {
            break;
        }
        if finalized == finalized_before_page {
            break;
        }
    }
    Ok(finalized)
}

pub async fn process_next_analysis<S>(
    store: &S,
    lease_owner: &str,
    now: OffsetDateTime,
    report_retention: Duration,
    policy: &AnalysisPolicy,
) -> Result<bool, GatewayError>
where
    S: AgentSessionAnalysisRepository
        + BudgetRepository
        + McpToolInvocationRepository
        + RequestLogRepository
        + Sync,
{
    let Some(queue) = store
        .claim_agent_analysis(lease_owner, now, now + Duration::minutes(1))
        .await?
    else {
        return Ok(false);
    };
    let current_versions = desired_versions_for_policy(policy);
    if queue.desired_versions.configuration_version != current_versions.configuration_version {
        enqueue_analysis_with_versions(
            store,
            queue.agent_session_id,
            "configuration_changed",
            &current_versions.configuration_version,
            now,
            &current_versions,
        )
        .await?;
        store
            .complete_agent_analysis(queue.queue_item_id, lease_owner, now)
            .await?;
        return Ok(true);
    }
    let report = generate_report(
        store,
        queue.agent_session_id,
        &queue.desired_versions,
        now,
        report_retention,
        policy,
    );
    tokio::pin!(report);
    let lease_interval = std::time::Duration::from_secs(20);
    let mut heartbeat =
        tokio::time::interval_at(tokio::time::Instant::now() + lease_interval, lease_interval);
    let result = loop {
        tokio::select! {
            result = &mut report => break result,
            _ = heartbeat.tick() => {
                let renewed_at = OffsetDateTime::now_utc();
                if !store
                    .renew_agent_analysis_lease(
                        queue.queue_item_id,
                        lease_owner,
                        renewed_at,
                        renewed_at + Duration::minutes(1),
                    )
                    .await?
                {
                    break Err(GatewayError::Internal(
                        "agent analysis lease was lost during report generation".to_string(),
                    ));
                }
            }
        }
    };
    match result {
        Ok(()) => {
            store
                .complete_agent_analysis(
                    queue.queue_item_id,
                    lease_owner,
                    OffsetDateTime::now_utc(),
                )
                .await?;
            Ok(true)
        }
        Err(error) => {
            let retry_at = (queue.attempts < queue.max_attempts)
                .then_some(now + Duration::seconds(i64::from(queue.attempts.max(1)) * 5));
            store
                .fail_agent_analysis(
                    queue.queue_item_id,
                    lease_owner,
                    &error.to_string(),
                    retry_at,
                    now,
                )
                .await?;
            Err(error)
        }
    }
}

pub(super) fn ensure_supported_versions(
    requested: &AgentAnalysisDesiredVersions,
) -> Result<(), GatewayError> {
    let mut supported_base = desired_versions();
    supported_base.score_maturity = agent_session_analysis::ScoreMaturity::Experimental;
    supported_base.calibration_approval_id = None;
    supported_base.configuration_version.clear();
    let mut requested_base = requested.clone();
    requested_base.score_maturity = agent_session_analysis::ScoreMaturity::Experimental;
    requested_base.calibration_approval_id = None;
    requested_base.configuration_version.clear();
    if requested_base != supported_base {
        return Err(GatewayError::Internal(format!(
            "unsupported agent analysis version tuple: requested {requested:?}, supported {supported_base:?}"
        )));
    }
    if requested.score_maturity == agent_session_analysis::ScoreMaturity::Calibrated
        && requested
            .calibration_approval_id
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err(GatewayError::Internal(
            "calibrated agent analysis requires an approval identity".to_string(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn desired_versions() -> AgentAnalysisDesiredVersions {
    let calibrated_score_enabled = std::env::var("AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
    let calibration_approval_id = std::env::var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| calibrated_score_enabled && !value.is_empty());
    let mut policy = AnalysisPolicy::default();
    if let Some(approval) = calibration_approval_id {
        policy.maturity = agent_session_analysis::ScoreMaturity::Calibrated;
        policy.calibration_approval_id = Some(approval);
    }
    desired_versions_for_policy(&policy)
}

#[must_use]
pub fn desired_versions_for_policy(policy: &AnalysisPolicy) -> AgentAnalysisDesiredVersions {
    let configuration_version = if policy.configuration_version.is_empty() {
        hash_identifier(
            &serde_json::to_string(policy).expect("analysis policy serialization is infallible"),
        )
    } else {
        policy.configuration_version.clone()
    };
    AgentAnalysisDesiredVersions {
        report_schema_version: agent_session_analysis::REPORT_SCHEMA_VERSION.to_string(),
        boundary_policy_version: SESSION_BOUNDARY_POLICY_VERSION.to_string(),
        observation_parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        analyzer_version: agent_session_analysis::ANALYZER_VERSION.to_string(),
        score_policy_version: agent_session_analysis::SCORE_POLICY_VERSION.to_string(),
        pricing_policy_version: PRICING_POLICY_VERSION.to_string(),
        cohort_version: COHORT_VERSION.to_string(),
        configuration_version,
        score_maturity: policy.maturity,
        calibration_approval_id: policy.calibration_approval_id.clone(),
    }
}
