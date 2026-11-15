//! `gateway rollout-guard`: watch live traffic on the gateway pods for a
//! window after a release, and fail when server errors rise.
//!
//! It reads each pod's cumulative counters from the management listener
//! (`/internal/traffic`) and sums what changed since it first saw each
//! process. Gateway-caused and upstream-caused server errors have separate
//! thresholds; 4xx responses never count. Below `min_requests` it passes,
//! because a few errors in little traffic say nothing about a release. As a
//! Helm `post-upgrade` hook its failure fails the release, so
//! `helm upgrade --rollback-on-failure` rolls back.

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use anyhow::{Context, bail};
use tokio::time::Instant;
use uuid::Uuid;

use crate::{cli::RolloutGuardArgs, http::traffic::TrafficSnapshot};

/// How long one pod gets to answer a poll.
const POLL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct GuardSettings {
    pub targets: Vec<String>,
    pub window: Duration,
    pub interval: Duration,
    pub min_requests: u64,
    pub max_gateway_error_percent: f64,
    pub max_upstream_error_percent: f64,
}

impl TryFrom<&RolloutGuardArgs> for GuardSettings {
    type Error = anyhow::Error;

    fn try_from(args: &RolloutGuardArgs) -> anyhow::Result<Self> {
        if args.interval_seconds == 0 {
            bail!("--interval-seconds must be at least 1");
        }
        for (flag, value) in [
            (
                "--max-gateway-error-percent",
                args.max_gateway_error_percent,
            ),
            (
                "--max-upstream-error-percent",
                args.max_upstream_error_percent,
            ),
        ] {
            if !(value > 0.0 && value <= 100.0) {
                bail!("{flag} must be above 0 and at most 100");
            }
        }
        Ok(Self {
            targets: args.targets.clone(),
            window: Duration::from_secs(args.window_seconds),
            interval: Duration::from_secs(args.interval_seconds),
            min_requests: args.min_requests,
            max_gateway_error_percent: args.max_gateway_error_percent,
            max_upstream_error_percent: args.max_upstream_error_percent,
        })
    }
}

/// Traffic counted during the window, across every process seen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WindowTotals {
    pub requests: u64,
    pub gateway_errors: u64,
    pub upstream_errors: u64,
}

impl WindowTotals {
    fn percent(errors: u64, requests: u64) -> f64 {
        if requests == 0 {
            return 0.0;
        }
        errors as f64 * 100.0 / requests as f64
    }

    /// Why the release fails, or `None` while it passes.
    #[must_use]
    pub fn failure(&self, settings: &GuardSettings) -> Option<String> {
        if self.requests < settings.min_requests {
            return None;
        }
        let gateway = Self::percent(self.gateway_errors, self.requests);
        if gateway >= settings.max_gateway_error_percent {
            return Some(format!(
                "gateway errors were {gateway:.1}% of {} requests ({} errors; limit {}%)",
                self.requests, self.gateway_errors, settings.max_gateway_error_percent
            ));
        }
        let upstream = Self::percent(self.upstream_errors, self.requests);
        if upstream >= settings.max_upstream_error_percent {
            return Some(format!(
                "upstream provider errors were {upstream:.1}% of {} requests ({} errors; limit {}%)",
                self.requests, self.upstream_errors, settings.max_upstream_error_percent
            ));
        }
        None
    }
}

/// First and latest snapshot of each gateway process, by instance id. A pod
/// that restarts is a new process with its own baseline.
#[derive(Debug, Default)]
pub struct TrafficWindow {
    instances: HashMap<Uuid, (TrafficSnapshot, TrafficSnapshot)>,
}

impl TrafficWindow {
    pub fn observe(&mut self, snapshot: TrafficSnapshot) {
        self.instances
            .entry(snapshot.instance_id)
            .and_modify(|(_, latest)| *latest = snapshot.clone())
            .or_insert_with(|| (snapshot.clone(), snapshot));
    }

    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    #[must_use]
    pub fn totals(&self) -> WindowTotals {
        self.instances
            .values()
            .fold(WindowTotals::default(), |mut totals, (first, latest)| {
                totals.requests += latest.requests.saturating_sub(first.requests);
                totals.gateway_errors += latest.gateway_errors.saturating_sub(first.gateway_errors);
                totals.upstream_errors +=
                    latest.upstream_errors.saturating_sub(first.upstream_errors);
                totals
            })
    }
}

/// Runs the guard to completion. An error means the release failed.
pub async fn run(args: &RolloutGuardArgs) -> anyhow::Result<()> {
    let settings = GuardSettings::try_from(args)?;
    println!(
        "rollout guard: watching {} for {}s (min {} requests, gateway errors < {}%, upstream errors < {}%)",
        settings.targets.join(", "),
        settings.window.as_secs(),
        settings.min_requests,
        settings.max_gateway_error_percent,
        settings.max_upstream_error_percent,
    );
    let totals = watch(&settings).await?;
    println!(
        "rollout guard passed: {} requests, {} gateway errors, {} upstream errors",
        totals.requests, totals.gateway_errors, totals.upstream_errors
    );
    Ok(())
}

/// Polls every `interval` until the window ends, failing as soon as the
/// error rate crosses a threshold.
pub async fn watch(settings: &GuardSettings) -> anyhow::Result<WindowTotals> {
    let client = reqwest::Client::builder()
        .timeout(POLL_TIMEOUT)
        .build()
        .context("failed building the rollout guard HTTP client")?;
    let deadline = Instant::now() + settings.window;
    let mut window = TrafficWindow::default();
    loop {
        for snapshot in poll(&client, &settings.targets).await {
            window.observe(snapshot);
        }
        let totals = window.totals();
        if let Some(reason) = totals.failure(settings) {
            bail!("rollout guard failed: {reason}");
        }
        let now = Instant::now();
        if now >= deadline {
            if window.instance_count() == 0 {
                bail!(
                    "rollout guard failed: no gateway answered at {} during the window",
                    settings.targets.join(", ")
                );
            }
            if totals.requests < settings.min_requests {
                println!(
                    "rollout guard: only {} requests in the window, below the minimum of {}; not judging the release",
                    totals.requests, settings.min_requests
                );
            }
            return Ok(totals);
        }
        println!(
            "rollout guard: {} pods, {} requests, {} gateway errors, {} upstream errors",
            window.instance_count(),
            totals.requests,
            totals.gateway_errors,
            totals.upstream_errors
        );
        tokio::time::sleep(settings.interval.min(deadline - now)).await;
    }
}

/// Every snapshot the targets' current addresses return. A headless Service
/// name resolves to one address per ready pod, so this re-resolves each time.
async fn poll(client: &reqwest::Client, targets: &[String]) -> Vec<TrafficSnapshot> {
    let mut snapshots = Vec::new();
    for target in targets {
        let addresses: Vec<SocketAddr> = match tokio::net::lookup_host(target.as_str()).await {
            Ok(addresses) => addresses.collect(),
            Err(error) => {
                eprintln!("rollout guard: cannot resolve {target}: {error}");
                continue;
            }
        };
        for address in addresses {
            match fetch(client, address).await {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(error) => eprintln!("rollout guard: {address}: {error:#}"),
            }
        }
    }
    snapshots
}

async fn fetch(client: &reqwest::Client, address: SocketAddr) -> anyhow::Result<TrafficSnapshot> {
    client
        .get(format!("http://{address}/internal/traffic"))
        .send()
        .await
        .context("request failed")?
        .error_for_status()
        .context("unexpected status")?
        .json()
        .await
        .context("unreadable traffic snapshot")
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use axum::http::StatusCode;
    use uuid::Uuid;

    use super::{GuardSettings, TrafficWindow, WindowTotals, watch};
    use crate::{
        http::traffic::{TrafficCounters, TrafficSnapshot, management_router},
        shutdown::Shutdown,
    };

    fn settings(targets: Vec<String>) -> GuardSettings {
        GuardSettings {
            targets,
            window: Duration::from_millis(400),
            interval: Duration::from_millis(50),
            min_requests: 50,
            max_gateway_error_percent: 5.0,
            max_upstream_error_percent: 25.0,
        }
    }

    fn snapshot(instance_id: Uuid, requests: u64, gateway: u64, upstream: u64) -> TrafficSnapshot {
        TrafficSnapshot {
            instance_id,
            version: "test".to_string(),
            started_at: 0,
            draining: false,
            requests,
            gateway_errors: gateway,
            upstream_errors: upstream,
        }
    }

    #[test]
    fn totals_count_only_traffic_after_each_process_was_first_seen() {
        let old_pod = Uuid::new_v4();
        let new_pod = Uuid::new_v4();
        let mut window = TrafficWindow::default();
        window.observe(snapshot(old_pod, 1_000, 40, 10));
        window.observe(snapshot(old_pod, 1_100, 41, 10));
        window.observe(snapshot(new_pod, 7, 0, 0));
        window.observe(snapshot(new_pod, 57, 3, 1));

        assert_eq!(
            window.totals(),
            WindowTotals {
                requests: 150,
                gateway_errors: 4,
                upstream_errors: 1,
            }
        );
    }

    #[test]
    fn judges_each_cause_against_its_own_threshold_once_traffic_is_enough() {
        let settings = settings(Vec::new());
        let quiet = WindowTotals {
            requests: 49,
            gateway_errors: 49,
            upstream_errors: 0,
        };
        assert_eq!(
            quiet.failure(&settings),
            None,
            "too little traffic to judge"
        );

        let gateway = WindowTotals {
            requests: 100,
            gateway_errors: 5,
            upstream_errors: 0,
        };
        assert!(
            gateway
                .failure(&settings)
                .is_some_and(|reason| reason.starts_with("gateway errors"))
        );

        let provider_blip = WindowTotals {
            requests: 100,
            gateway_errors: 4,
            upstream_errors: 24,
        };
        assert_eq!(provider_blip.failure(&settings), None);

        let provider_broken = WindowTotals {
            requests: 100,
            gateway_errors: 0,
            upstream_errors: 25,
        };
        assert!(
            provider_broken
                .failure(&settings)
                .is_some_and(|reason| reason.starts_with("upstream provider errors"))
        );
    }

    async fn serve_management(counters: Arc<TrafficCounters>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind management listener");
        let address = listener.local_addr().expect("listener address");
        tokio::spawn(async move {
            axum::serve(listener, management_router(counters, Shutdown::new()))
                .await
                .expect("management listener");
        });
        address.to_string()
    }

    #[tokio::test]
    async fn fails_when_gateway_errors_rise_during_the_window() {
        let counters = Arc::new(TrafficCounters::new());
        let target = serve_management(counters.clone()).await;
        let traffic = tokio::spawn({
            let counters = counters.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                for request in 0..100 {
                    let status = if request % 10 == 0 {
                        StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        StatusCode::OK
                    };
                    counters.record(status, false);
                }
            }
        });

        let error = watch(&settings(vec![target]))
            .await
            .expect_err("10% gateway errors fail the release");
        traffic.await.expect("traffic task");
        assert!(
            error.to_string().contains("gateway errors were 10.0%"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn passes_quiet_or_healthy_traffic_and_fails_when_nothing_answers() {
        let counters = Arc::new(TrafficCounters::new());
        let target = serve_management(counters.clone()).await;
        for _ in 0..200 {
            counters.record(StatusCode::OK, false);
        }
        counters.record(StatusCode::NOT_FOUND, false);
        let totals = watch(&settings(vec![target]))
            .await
            .expect("traffic before the window is not judged");
        assert_eq!(totals.requests, 0);

        let unreachable = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind")
            .local_addr()
            .expect("address")
            .to_string();
        let error = watch(&settings(vec![unreachable]))
            .await
            .expect_err("no answers fails the guard");
        assert!(error.to_string().contains("no gateway answered"), "{error}");
    }
}
