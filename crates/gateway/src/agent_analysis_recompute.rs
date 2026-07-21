use anyhow::{Context, bail};
use gateway::{cli::RecomputeAgentAnalysisArgs, config::GatewayConfig};
use gateway_core::{AgentSessionAnalysisRepository, AgentTaskListQuery};
use gateway_service::enqueue_agent_analysis;
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
        let mut tasks = Vec::with_capacity(args.limit as usize);
        let mut page = 1;
        while tasks.len() < args.limit as usize {
            let remaining = args.limit as usize - tasks.len();
            let page_size = u32::try_from(remaining.min(200)).expect("bounded page size");
            let result = store
                .list_agent_tasks(&AgentTaskListQuery {
                    page,
                    page_size,
                    ..AgentTaskListQuery::default()
                })
                .await
                .context("failed to list agent tasks")?;
            let item_count = result.items.len();
            tasks.extend(result.items.into_iter().map(|trace| trace.task));
            if item_count < page_size as usize {
                break;
            }
            page = page.saturating_add(1);
        }
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
