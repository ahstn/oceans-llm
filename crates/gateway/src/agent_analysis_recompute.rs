use anyhow::{Context, bail};
use gateway::{cli::RecomputeAgentAnalysisArgs, config::GatewayConfig};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentSessionListQuery, AgentSessionReportRepository,
    AgentSessionTraceRecord, AgentSessionTraceRepository, SessionLifecycleState,
};
use gateway_service::{desired_versions_for_policy, enqueue_agent_analysis_with_versions};
use gateway_store::AnyStore;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{database_options, maybe_run_migrations};

pub async fn run_command(
    config: &GatewayConfig,
    args: RecomputeAgentAnalysisArgs,
) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    maybe_run_migrations(&database_options, true).await?;
    let store = AnyStore::connect(&database_options)
        .await
        .context("failed to initialize gateway store")?;
    let analysis_settings = config.agent_analysis.resolve()?;
    let desired = desired_versions_for_policy(&analysis_settings.policy);

    let sessions = if let Some(session_id) = args.session_id {
        let session_id = Uuid::parse_str(&session_id).context("--session-id must be a UUID")?;
        let Some(trace) = store
            .load_agent_session_trace(session_id)
            .await
            .context("failed to load agent session")?
        else {
            bail!("agent session `{session_id}` was not found");
        };
        vec![trace.session]
    } else {
        let desired = desired.clone();
        let mut sessions = Vec::with_capacity(args.limit as usize);
        let mut page = 1;
        let page_size = gateway_core::MAX_AGENT_SESSION_PAGE_SIZE;
        while sessions.len() < args.limit as usize {
            let result = store
                .list_agent_sessions(&AgentSessionListQuery {
                    lifecycle: Some(SessionLifecycleState::Finalized),
                    page,
                    page_size,
                    ..AgentSessionListQuery::default()
                })
                .await
                .context("failed to list agent sessions")?;
            let item_count = result.items.len();
            sessions.extend(
                result
                    .items
                    .into_iter()
                    .filter(|trace| !analysis_is_current(trace, &desired))
                    .map(|trace| trace.session),
            );
            if item_count < page_size as usize {
                break;
            }
            page = page.saturating_add(1);
        }
        sessions.truncate(args.limit as usize);
        sessions
    };

    let now = OffsetDateTime::now_utc();
    let matched_count = sessions.len();
    let mut enqueued_count = 0_usize;
    for session in sessions {
        let dedupe_key = format!(
            "{}:{}",
            session.agent_session_id,
            session.input_watermark_at.unix_timestamp_nanos()
        );
        if enqueue_agent_analysis_with_versions(
            &store,
            session.agent_session_id,
            "manual_recompute",
            &dedupe_key,
            now,
            &desired,
        )
        .await
        .context("failed to enqueue agent analysis")?
        {
            enqueued_count += 1;
        }
    }

    println!("matched_count: {matched_count}");
    println!("enqueued_count: {enqueued_count}");
    Ok(())
}

fn analysis_is_current(
    trace: &AgentSessionTraceRecord,
    desired: &AgentAnalysisDesiredVersions,
) -> bool {
    let Some(analysis) = trace.latest_analysis.as_ref() else {
        return false;
    };
    !analysis.stale
        && analysis.input_watermark_at == trace.session.input_watermark_at
        && analysis.boundary_policy_version == desired.boundary_policy_version
        && analysis.observation_parser_version == desired.observation_parser_version
        && analysis.pricing_policy_version == desired.pricing_policy_version
        && analysis.cohort_version == desired.cohort_version
        && analysis.report.report_schema_version == desired.report_schema_version
        && analysis.report.observation_parser_version == desired.observation_parser_version
        && analysis.report.analyzer_version == desired.analyzer_version
        && analysis.report.score_policy_version == desired.score_policy_version
        && analysis.report.maturity == desired.score_maturity
        && analysis.report.calibration_approval_id == desired.calibration_approval_id
        && analysis.report.configuration_version == desired.configuration_version
}
