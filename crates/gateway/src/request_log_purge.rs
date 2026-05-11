use std::{sync::Arc, time::Duration};

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use gateway_core::{RequestLogPurgeResult, RequestLogRetentionWindow};
use gateway_service::{GatewayService, RequestLogging, WeightedRoutePlanner};
use gateway_store::{AnyStore, check_migrations_with_options};

use gateway::{
    cli::PurgeRequestLogsArgs,
    config::{GatewayConfig, RequestLogPurgeConfig},
};

use crate::{database_options, maybe_run_migrations};

pub async fn run_command(config: &GatewayConfig, args: PurgeRequestLogsArgs) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    if args.dry_run {
        ensure_no_pending_migrations_for_dry_run(&database_options).await?;
    } else {
        maybe_run_migrations(&database_options, true).await?;
    }

    let store = Arc::new(
        AnyStore::connect(&database_options)
            .await
            .context("failed to initialize gateway store")?,
    );
    let request_logging = RequestLogging::new(store);
    let result = request_logging
        .purge_request_logs(args.retention, args.dry_run)
        .await
        .context("failed to purge request logs")?;
    println!("cutoff: {}", result.cutoff);
    println!("dry_run: {}", result.dry_run);
    println!("matched_count: {}", result.matched_count);
    println!("deleted_count: {}", result.deleted_count);
    Ok(())
}

pub fn spawn_loop(
    service: Arc<GatewayService<AnyStore, WeightedRoutePlanner>>,
    config: &RequestLogPurgeConfig,
) {
    if !config.enabled {
        return;
    }

    let schedule = match parse_schedule(&config.schedule) {
        Ok(schedule) => schedule,
        Err(error) => {
            tracing::warn!(error = %error, "request log purge schedule is invalid");
            return;
        }
    };
    let retention = config.retention;

    tokio::spawn(async move {
        let mut last_started_at: Option<DateTime<Utc>> = None;

        loop {
            let now = Utc::now();
            let delay = delay_until_next_cron_run(&schedule, now);
            tokio::time::sleep(delay).await;

            let started_at = Utc::now();
            if !daily_purge_guard_allows_run(started_at, last_started_at) {
                tracing::warn!("request log purge skipped by daily runtime guard");
                continue;
            }
            last_started_at = Some(started_at);

            if let Err(error) =
                purge_request_logs_from_service(service.clone(), retention, false).await
            {
                tracing::warn!(error = %error, "background request log purge failed");
            }
        }
    });
}

async fn ensure_no_pending_migrations_for_dry_run(
    database_options: &gateway_store::StoreConnectionOptions,
) -> anyhow::Result<()> {
    let status = check_migrations_with_options(database_options)
        .await
        .context("failed to check database migrations before request-log purge dry run")?;
    if status.entries.iter().any(|entry| !entry.applied) {
        bail!("pending database migrations; run `gateway migrate --apply` before using --dry-run");
    }
    Ok(())
}

async fn purge_request_logs_from_service(
    service: Arc<GatewayService<AnyStore, WeightedRoutePlanner>>,
    retention: RequestLogRetentionWindow,
    dry_run: bool,
) -> anyhow::Result<RequestLogPurgeResult> {
    service
        .purge_request_logs(retention, dry_run)
        .await
        .context("failed to purge request logs")
}

fn parse_schedule(schedule: &str) -> anyhow::Result<cron::Schedule> {
    format!("0 {}", schedule.trim())
        .parse()
        .with_context(|| format!("invalid request log purge schedule `{schedule}`"))
}

fn delay_until_next_cron_run(schedule: &cron::Schedule, now: DateTime<Utc>) -> Duration {
    let next = schedule
        .after(&now)
        .find(|candidate| *candidate > now)
        .expect("validated request log purge schedule should have a next run");
    next.signed_duration_since(now)
        .to_std()
        .expect("next request log purge run should be after the current time")
}

fn daily_purge_guard_allows_run(
    now: DateTime<Utc>,
    last_started_at: Option<DateTime<Utc>>,
) -> bool {
    match last_started_at {
        Some(last_started_at) => now.date_naive() > last_started_at.date_naive(),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::TimeZone;

    use super::{daily_purge_guard_allows_run, delay_until_next_cron_run, parse_schedule};

    #[test]
    fn daily_guard_blocks_multiple_runs_on_same_utc_day() {
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 5, 10, 12, 0, 0)
            .single()
            .expect("datetime");

        assert!(daily_purge_guard_allows_run(now, None));
        assert!(!daily_purge_guard_allows_run(
            now,
            Some(now - chrono::Duration::hours(1))
        ));
        assert!(daily_purge_guard_allows_run(
            now,
            Some(now - chrono::Duration::hours(23))
        ));
    }

    #[test]
    fn cron_delay_uses_standard_five_field_schedule() {
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 5, 10, 12, 0, 0)
            .single()
            .expect("datetime");

        let schedule = parse_schedule("0 0 * * *").expect("schedule");
        let delay = delay_until_next_cron_run(&schedule, now);

        assert_eq!(delay, Duration::from_secs(12 * 60 * 60));
    }
}
