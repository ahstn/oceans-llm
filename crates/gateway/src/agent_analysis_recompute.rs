use anyhow::{Context, bail};
use gateway::{cli::RecomputeAgentAnalysisArgs, config::GatewayConfig};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentSessionAnalysisRepository, AgentTaskListQuery,
    AgentTaskTraceRecord, TaskLifecycleState,
};
use gateway_service::{desired_versions, enqueue_agent_analysis};
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

    let tasks = if let Some(task_id) = args.task_id {
        let task_id = Uuid::parse_str(&task_id).context("--task-id must be a UUID")?;
        let Some(trace) = store
            .load_agent_task_trace(task_id)
            .await
            .context("failed to load agent task")?
        else {
            bail!("agent task `{task_id}` was not found");
        };
        vec![trace.task]
    } else {
        let desired = desired_versions();
        let mut tasks = Vec::with_capacity(args.limit as usize);
        let mut page = 1;
        let page_size = gateway_core::MAX_AGENT_TASK_PAGE_SIZE;
        while tasks.len() < args.limit as usize {
            let result = store
                .list_agent_tasks(&AgentTaskListQuery {
                    lifecycle: Some(TaskLifecycleState::Finalized),
                    page,
                    page_size,
                    ..AgentTaskListQuery::default()
                })
                .await
                .context("failed to list agent tasks")?;
            let item_count = result.items.len();
            tasks.extend(
                result
                    .items
                    .into_iter()
                    .filter(|trace| !analysis_is_current(trace, &desired))
                    .map(|trace| trace.task),
            );
            if item_count < page_size as usize {
                break;
            }
            page = page.saturating_add(1);
        }
        tasks.truncate(args.limit as usize);
        tasks
    };

    let now = OffsetDateTime::now_utc();
    let matched_count = tasks.len();
    let mut enqueued_count = 0_usize;
    for task in tasks {
        let dedupe_key = format!(
            "{}:{}",
            task.agent_task_id,
            task.input_watermark_at.unix_timestamp_nanos()
        );
        if enqueue_agent_analysis(
            &store,
            task.agent_task_id,
            "manual_recompute",
            &dedupe_key,
            now,
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
    trace: &AgentTaskTraceRecord,
    desired: &AgentAnalysisDesiredVersions,
) -> bool {
    let Some(analysis) = trace.latest_analysis.as_ref() else {
        return false;
    };
    !analysis.stale
        && analysis.input_watermark_at == trace.task.input_watermark_at
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
}
