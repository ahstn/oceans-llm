//! Ends MCP server-to-client event streams when their caller can no longer
//! open them.
//!
//! The grant check runs once, when a stream opens, and a stream can outlive it
//! by hours. One shared watcher rechecks every open stream each interval: the
//! callers' API keys are read in one batched query and checked as
//! authentication checks them, and each distinct caller and server pair is
//! checked once however many streams share it. Database work grows with the
//! number of distinct callers, not with the number of open streams, and no
//! stream runs a timer of its own.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::Body,
    http::{Response, header::CONTENT_TYPE},
};
use gateway_core::{AuthenticatedApiKey, SCOPE_MCP_PROXY};
use gateway_service::McpAccess;
use gateway_store::AnyStore;
use tokio::sync::Notify;
use uuid::Uuid;

/// How often the watcher rechecks open streams.
pub const STREAM_REACH_RECHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Who opened an event stream, as needed to recheck them while it is open.
#[derive(Clone)]
pub(crate) struct StreamCaller {
    pub(crate) auth: AuthenticatedApiKey,
    pub(crate) credential: StreamCredential,
    pub(crate) server_key: String,
    pub(crate) mcp_server_id: Uuid,
}

/// The credential an event stream was opened with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamCredential {
    /// A `gwk_` API key, read again by its ID on each recheck: it must still
    /// authenticate, with an MCP scope and the server in its server list.
    ApiKey,
    /// An OAuth access token, whose revocation and expiry
    /// `bound_to_oauth_access` watches.
    OauthToken,
}

impl StreamCaller {
    /// The caller of an event stream opened with `auth`.
    pub(crate) fn of(auth: &AuthenticatedApiKey, server_key: &str, mcp_server_id: Uuid) -> Self {
        Self {
            auth: auth.clone(),
            credential: if auth.oauth.is_some() {
                StreamCredential::OauthToken
            } else {
                StreamCredential::ApiKey
            },
            server_key: server_key.to_ascii_lowercase(),
            mcp_server_id,
        }
    }
}

/// Tracks open event streams and ends those whose caller lost reach.
pub struct McpStreamReachWatcher {
    store: Arc<AnyStore>,
    every: Duration,
    streams: Mutex<HashMap<u64, WatchedStream>>,
    next_id: AtomicU64,
    recheck_loop: OnceLock<()>,
    /// Caller and server pairs checked by the last recheck, for tests.
    last_pairs_checked: AtomicUsize,
}

struct WatchedStream {
    caller: Arc<StreamCaller>,
    ended: Arc<Notify>,
}

/// Registration for one open stream; dropping it stops the watch.
struct StreamWatch {
    watcher: Weak<McpStreamReachWatcher>,
    id: u64,
    ended: Arc<Notify>,
}

impl Drop for StreamWatch {
    fn drop(&mut self) {
        if let Some(watcher) = self.watcher.upgrade() {
            watcher.lock_streams().remove(&self.id);
        }
    }
}

impl McpStreamReachWatcher {
    pub fn new(store: Arc<AnyStore>, every: Duration) -> Arc<Self> {
        Arc::new(Self {
            store,
            every,
            streams: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            recheck_loop: OnceLock::new(),
            last_pairs_checked: AtomicUsize::new(0),
        })
    }

    fn lock_streams(&self) -> std::sync::MutexGuard<'_, HashMap<u64, WatchedStream>> {
        self.streams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn watch(self: &Arc<Self>, caller: StreamCaller) -> StreamWatch {
        self.start_recheck_loop();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let ended = Arc::new(Notify::new());
        self.lock_streams().insert(
            id,
            WatchedStream {
                caller: Arc::new(caller),
                ended: ended.clone(),
            },
        );
        StreamWatch {
            watcher: Arc::downgrade(self),
            id,
            ended,
        }
    }

    /// Starts the shared loop on first use. It holds only a weak reference,
    /// so it stops once the gateway state is dropped.
    fn start_recheck_loop(self: &Arc<Self>) {
        self.recheck_loop.get_or_init(|| {
            let watcher = Arc::downgrade(self);
            let every = self.every;
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(every);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let Some(watcher) = watcher.upgrade() else {
                        return;
                    };
                    watcher.recheck().await;
                }
            });
        });
    }

    /// Rechecks every open stream once and ends those whose caller can no
    /// longer open them. A check that cannot be answered ends the streams it
    /// covers, so a store outage never extends access; their clients
    /// reconnect and are checked afresh.
    async fn recheck(&self) {
        let watched: Vec<(Arc<StreamCaller>, Arc<Notify>)> = self
            .lock_streams()
            .values()
            .map(|stream| (stream.caller.clone(), stream.ended.clone()))
            .collect();
        if watched.is_empty() {
            self.last_pairs_checked.store(0, Ordering::Relaxed);
            return;
        }
        let access = McpAccess::new(self.store.clone());
        let key_ids: Vec<Uuid> = watched
            .iter()
            .filter(|(caller, _)| caller.credential == StreamCredential::ApiKey)
            .map(|(caller, _)| caller.auth.id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let usable = if key_ids.is_empty() {
            HashMap::new()
        } else {
            match access.usable_api_keys(&key_ids).await {
                Ok(usable) => usable,
                Err(error) => {
                    tracing::warn!(error = %error, "MCP stream key recheck failed; ending the streams");
                    for (_, ended) in &watched {
                        ended.notify_one();
                    }
                    return;
                }
            }
        };
        // Each distinct caller and server is checked once, however many
        // streams it holds open.
        let mut reach: HashMap<(Uuid, Uuid), bool> = HashMap::new();
        for (caller, ended) in &watched {
            let auth = match caller.credential {
                StreamCredential::ApiKey => match usable.get(&caller.auth.id) {
                    Some(auth)
                        if auth.access.has_scope(SCOPE_MCP_PROXY)
                            && auth.require_mcp_server(&caller.server_key).is_ok() =>
                    {
                        auth
                    }
                    _ => {
                        ended.notify_one();
                        continue;
                    }
                },
                StreamCredential::OauthToken => &caller.auth,
            };
            let pair = (caller.auth.id, caller.mcp_server_id);
            let reached = match reach.get(&pair) {
                Some(reached) => *reached,
                None => {
                    let reached = match access.has_server_grant(auth, caller.mcp_server_id).await {
                        Ok(reached) => reached,
                        Err(error) => {
                            tracing::warn!(error = %error, "MCP stream reach recheck failed; ending the stream");
                            false
                        }
                    };
                    reach.insert(pair, reached);
                    reached
                }
            };
            if !reached {
                ended.notify_one();
            }
        }
        self.last_pairs_checked
            .store(reach.len(), Ordering::Relaxed);
    }

    /// Streams being watched now.
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        self.lock_streams().len()
    }

    /// Caller and server pairs the last recheck checked.
    #[cfg(test)]
    pub(crate) fn last_pairs_checked(&self) -> usize {
        self.last_pairs_checked.load(Ordering::Relaxed)
    }
}

/// Ends `response`'s body once `caller` can no longer open the stream: their
/// key was revoked or lost its MCP scope, their team no longer owns a
/// team-only server, the server was narrowed away from them, or their grants
/// are gone.
pub(crate) fn bound_to_reach(
    watcher: &Arc<McpStreamReachWatcher>,
    caller: StreamCaller,
    response: Response<Body>,
) -> Response<Body> {
    use futures_util::StreamExt;
    let watch = watcher.watch(caller);
    let ended = async move {
        watch.ended.notified().await;
        drop(watch);
    };
    let (parts, body) = response.into_parts();
    let body = body.into_data_stream().take_until(Box::pin(ended));
    Response::from_parts(parts, Body::from_stream(body))
}

/// Whether `response` streams events, by its media type, matched without case
/// as the upstream proxy matches it.
pub(crate) fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}
