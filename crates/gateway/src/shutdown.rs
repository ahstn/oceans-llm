//! Graceful shutdown: stop taking work, let in-flight requests finish for a
//! bounded time, then end what remains and wait for its accounting.
//!
//! On the first SIGTERM or Ctrl-C the gateway starts draining. `/readyz`
//! reports it, the listener stops accepting connections, and MCP event streams
//! end so their clients reconnect elsewhere. In-flight requests and inference
//! streams continue until `server.shutdown_drain_seconds` passes or a second
//! signal arrives. Then the cutoff ends the remaining inference streams with
//! an error. Connections get five seconds to close before they are aborted and
//! joined, dropping even bodies blocked by a client that stopped reading.
//! Only then do tracked handlers, usage and request-log writes get up to ten
//! seconds to finish. These 5 + 10 seconds fit the chart's 20-second allowance
//! after its preStop (10 seconds) and drain (300 seconds), within 330 seconds.

use std::{future::Future, time::Duration};

use axum::{Extension, Router, extract::ConnectInfo};
use hyper::rt::Executor;
use hyper_util::{rt::TokioIo, server::conn::auto::Builder, service::TowerToHyperService};
use tokio::net::TcpListener;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// How long connections get to close after the cutoff ends their streams.
const CUTOFF_GRACE: Duration = Duration::from_secs(5);
/// How long tracked usage and request-log writes get once serving stops.
const FINALIZATION_GRACE: Duration = Duration::from_secs(10);

/// Hyper also spawns HTTP/2 stream and upgrade tasks through its executor.
/// Track and cancel those with the connections, rather than leaving body
/// ownership in detached Tokio tasks.
#[derive(Clone, Default)]
struct ConnectionExecutor {
    tasks: TaskTracker,
    abort: CancellationToken,
}

impl<F> Executor<F> for ConnectionExecutor
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    fn execute(&self, future: F) {
        let abort = self.abort.clone();
        self.tasks.spawn(async move {
            tokio::select! {
                biased;
                () = abort.cancelled() => {}
                _ = future => {}
            }
        });
    }
}

/// The gateway's shutdown state, shared by the listener, readiness, and
/// every response that outlives its handler. Cloning shares the state.
#[derive(Clone, Debug, Default)]
pub struct Shutdown {
    draining: CancellationToken,
    cutoff: CancellationToken,
    tasks: TaskTracker,
}

impl Shutdown {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the gateway has stopped taking new work.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.draining.is_cancelled()
    }

    pub fn begin_drain(&self) {
        self.draining.cancel();
    }

    /// Ends the drain: streams still open stop with an error.
    pub fn cut_off(&self) {
        self.draining.cancel();
        self.cutoff.cancel();
    }

    /// Resolves when draining starts.
    pub async fn drain_started(self) {
        self.draining.cancelled_owned().await;
    }

    /// Resolves at the cutoff.
    pub async fn cut_off_reached(&self) {
        self.cutoff.cancelled().await;
    }

    #[must_use]
    pub fn is_cut_off(&self) -> bool {
        self.cutoff.is_cancelled()
    }

    /// Tasks the gateway waits for before it exits, such as stream usage
    /// accounting and request logging.
    #[must_use]
    pub fn tasks(&self) -> &TaskTracker {
        &self.tasks
    }

    /// Owns every accepted connection, including its response bodies. Before
    /// returning, closes and joins even connections blocked on socket writes,
    /// so their drop finalizers are registered before `finish_tasks` starts.
    /// Uses axum's TCP accept retry and resource-error backoff policy.
    pub async fn serve(&self, mut listener: TcpListener, app: Router) -> std::io::Result<()> {
        let connections = ConnectionExecutor::default();
        loop {
            tokio::select! {
                biased;
                () = self.draining.cancelled() => break,
                (socket, peer) = axum::serve::Listener::accept(&mut listener) => {
                    let service = app.clone().layer(Extension(ConnectInfo(peer)));
                    let shutdown = self.clone();
                    let executor = connections.clone();
                    connections.execute(async move {
                        let mut builder = Builder::new(executor);
                        builder.http2().enable_connect_protocol();
                        let connection = builder.serve_connection_with_upgrades(
                            TokioIo::new(socket), TowerToHyperService::new(service),
                        );
                        tokio::pin!(connection);
                        let result = tokio::select! {
                            result = &mut connection => result,
                            () = shutdown.draining.cancelled() => {
                                connection.as_mut().graceful_shutdown();
                                connection.await
                            }
                        };
                        if let Err(error) = result {
                            tracing::debug!(%error, "connection closed with an error");
                        }
                    });
                }
            }
        }
        drop(listener);
        connections.tasks.close();
        tokio::select! {
            () = connections.tasks.wait() => {}
            () = async {
                self.cut_off_reached().await;
                tokio::time::sleep(CUTOFF_GRACE).await;
            } => {
                tracing::warn!("connections were still open after the shutdown cutoff; closing them");
                connections.abort.cancel();
                connections.tasks.wait().await;
            }
        }
        Ok(())
    }

    /// Waits up to [`FINALIZATION_GRACE`] for tracked tasks. Returns whether
    /// they all finished.
    pub async fn finish_tasks(&self) -> bool {
        self.tasks.close();
        tokio::time::timeout(FINALIZATION_GRACE, self.tasks.wait())
            .await
            .is_ok()
    }

    /// Starts draining on the first SIGTERM or Ctrl-C and cuts off after
    /// `drain`, or at once on a second signal. Handlers are installed before
    /// this returns, so a signal that arrives during startup is not lost.
    pub fn listen_for_signals(&self, drain: Duration) -> anyhow::Result<()> {
        let mut signals = ShutdownSignals::install()?;
        let shutdown = self.clone();
        tokio::spawn(async move {
            signals.next().await;
            tracing::info!(
                drain_seconds = drain.as_secs(),
                "shutdown signal received; draining in-flight requests"
            );
            shutdown.begin_drain();
            tokio::select! {
                () = tokio::time::sleep(drain) => {
                    tracing::warn!("shutdown drain limit reached; ending open streams");
                }
                () = signals.next() => {
                    tracing::warn!("second shutdown signal received; ending open streams now");
                }
            }
            shutdown.cut_off();
        });
        Ok(())
    }
}

/// SIGTERM, which Kubernetes and container runtimes send, and Ctrl-C.
struct ShutdownSignals {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    fn install() -> anyhow::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        })
    }

    async fn next(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Shutdown;

    #[tokio::test(start_paused = true)]
    async fn finish_tasks_waits_for_tracked_work_but_not_forever() {
        let shutdown = Shutdown::new();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        shutdown.tasks().spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = done_tx.send(());
        });
        assert!(shutdown.finish_tasks().await);
        done_rx
            .await
            .expect("tracked task finished before shutdown returned");

        let stuck = Shutdown::new();
        stuck.tasks().spawn(std::future::pending::<()>());
        assert!(!stuck.finish_tasks().await);
    }
}
