//! Ends OAuth callers' MCP event streams when their access does.
//!
//! An access token is checked once, when a request arrives. A response that
//! streams for hours would otherwise outlive the token, a revoked grant, or a
//! sign-out, so each stream closes at its token's expiry, and as soon as a
//! recheck finds the token's grant, session, or user no longer live.
//!
//! One shared watcher does the rechecks: every interval it looks up all open
//! streams' tokens in one batched query, so database work grows with the
//! interval rather than with the number of open streams.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{
    body::Body,
    http::{Response, header::CONTENT_TYPE},
};
use futures_util::StreamExt;
use gateway_core::{AuthenticatedApiKey, GatewayOauthRepository, OauthAccessContext};
use gateway_store::AnyStore;
use time::OffsetDateTime;
use tokio::sync::Notify;

use crate::http::state::AppState;

/// How often the watcher rechecks open streams' grants, sessions, and users.
pub const OAUTH_STREAM_RECHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Tracks the open OAuth event streams and ends those whose access stopped.
pub struct OauthStreamWatcher {
    store: Arc<AnyStore>,
    every: Duration,
    streams: Mutex<HashMap<u64, WatchedStream>>,
    next_id: AtomicU64,
    recheck_loop: OnceLock<()>,
}

struct WatchedStream {
    token_hash: String,
    ended: Arc<Notify>,
}

/// Registration for one open stream; dropping it stops the watch.
struct StreamWatch {
    watcher: Weak<OauthStreamWatcher>,
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

impl OauthStreamWatcher {
    pub fn new(store: Arc<AnyStore>, every: Duration) -> Arc<Self> {
        Arc::new(Self {
            store,
            every,
            streams: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            recheck_loop: OnceLock::new(),
        })
    }

    fn lock_streams(&self) -> std::sync::MutexGuard<'_, HashMap<u64, WatchedStream>> {
        self.streams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn watch(self: &Arc<Self>, token_hash: String) -> StreamWatch {
        self.start_recheck_loop();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let ended = Arc::new(Notify::new());
        self.lock_streams().insert(
            id,
            WatchedStream {
                token_hash,
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

    /// Ends every open stream whose token is no longer live. A store error
    /// keeps the streams open until the next recheck; expiry still applies.
    async fn recheck(&self) {
        let watched: Vec<(String, Arc<Notify>)> = self
            .lock_streams()
            .values()
            .map(|stream| (stream.token_hash.clone(), stream.ended.clone()))
            .collect();
        if watched.is_empty() {
            return;
        }
        let hashes: Vec<String> = watched
            .iter()
            .map(|(hash, _)| hash.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        match self
            .store
            .live_gateway_oauth_access_tokens(&hashes, OffsetDateTime::now_utc())
            .await
        {
            Ok(live) => {
                let live: HashSet<String> = live.into_iter().collect();
                for (hash, ended) in watched {
                    if !live.contains(&hash) {
                        ended.notify_one();
                    }
                }
            }
            Err(error) => tracing::warn!(error = %error, "MCP OAuth stream recheck failed"),
        }
    }
}

/// Bounds an event-stream `response` for an OAuth caller by its access.
/// API-key callers and other responses are returned unchanged.
pub(crate) fn bound_to_oauth_access(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    response: Response<Body>,
) -> Response<Body> {
    match auth.oauth.as_ref() {
        Some(oauth) if is_event_stream(&response) => {
            bound_by(&state.oauth_streams, oauth, response)
        }
        _ => response,
    }
}

fn bound_by(
    watcher: &Arc<OauthStreamWatcher>,
    oauth: &OauthAccessContext,
    response: Response<Body>,
) -> Response<Body> {
    let watch = watcher.watch(oauth.token_hash.clone());
    let remaining =
        Duration::try_from(oauth.expires_at - OffsetDateTime::now_utc()).unwrap_or(Duration::ZERO);
    let ended = async move {
        tokio::select! {
            () = tokio::time::sleep(remaining) => {}
            () = watch.ended.notified() => {}
        }
        drop(watch);
    };
    let (parts, body) = response.into_parts();
    let body = body.into_data_stream().take_until(Box::pin(ended));
    Response::from_parts(parts, Body::from_stream(body))
}

/// Classifies the media type exactly as the upstream proxy does (case
/// insensitive), so every response it streams is bounded here.
fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode};
    use gateway_core::McpOauthResource;
    use gateway_service::McpClientOauthService;

    use super::super::flow_tests::Harness;
    use super::*;

    fn endless_event_stream() -> Response<Body> {
        endless_stream_with_type("text/event-stream")
    }

    fn endless_stream_with_type(content_type: &str) -> Response<Body> {
        let chunks = futures_util::stream::once(async {
            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"data: hello\n\n"))
        })
        .chain(futures_util::stream::pending());
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, content_type)
            .body(Body::from_stream(chunks))
            .unwrap()
    }

    async fn oauth_caller(harness: &Harness) -> AuthenticatedApiKey {
        let tokens = harness.issue_tokens(&[]).await;
        McpClientOauthService::new(
            harness.state.store.clone(),
            harness.state.mcp_client_oauth.clone().unwrap(),
        )
        .authenticate_access_token(
            tokens["access_token"].as_str().unwrap(),
            &McpOauthResource::Aggregate,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap()
    }

    async fn read_body(response: Response<Body>) -> Vec<u8> {
        tokio::time::timeout(
            Duration::from_secs(5),
            to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("the stream should have ended")
        .unwrap()
        .to_vec()
    }

    #[tokio::test]
    async fn streams_end_when_the_access_token_expires() {
        let harness = Harness::new().await;
        let mut auth = oauth_caller(&harness).await;
        auth.oauth.as_mut().unwrap().expires_at =
            OffsetDateTime::now_utc() + time::Duration::milliseconds(50);
        let response = bound_to_oauth_access(&harness.state, &auth, endless_event_stream());
        assert_eq!(read_body(response).await, b"data: hello\n\n");
        // Media types are case-insensitive, and parameters may follow.
        let response = bound_to_oauth_access(
            &harness.state,
            &auth,
            endless_stream_with_type("Text/Event-Stream; charset=utf-8"),
        );
        assert_eq!(read_body(response).await, b"data: hello\n\n");
    }

    #[tokio::test]
    async fn one_recheck_ends_only_the_streams_whose_grant_was_revoked() {
        let harness = Harness::new().await;
        let revoked = oauth_caller(&harness).await.oauth.unwrap();
        let kept = oauth_caller(&harness).await.oauth.unwrap();
        let watcher =
            OauthStreamWatcher::new(harness.state.store.clone(), Duration::from_millis(20));
        let revoked_stream = bound_by(&watcher, &revoked, endless_event_stream());
        let kept_stream = bound_by(&watcher, &kept, endless_event_stream());
        assert_eq!(watcher.lock_streams().len(), 2);
        harness
            .state
            .store
            .revoke_gateway_oauth_grant(revoked.grant_id, "user_revoked", OffsetDateTime::now_utc())
            .await
            .unwrap();
        assert_eq!(read_body(revoked_stream).await, b"data: hello\n\n");

        let still_open = tokio::time::timeout(
            Duration::from_millis(200),
            to_bytes(kept_stream.into_body(), usize::MAX),
        )
        .await;
        assert!(still_open.is_err(), "the other grant's stream stays open");
        // Both streams are gone, so nothing is left to recheck.
        assert!(watcher.lock_streams().is_empty());
    }

    #[tokio::test]
    async fn api_key_callers_and_plain_responses_are_untouched() {
        let harness = Harness::new().await;
        let mut auth = oauth_caller(&harness).await;
        let json = Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let json = bound_to_oauth_access(&harness.state, &auth, json);
        assert_eq!(read_body(json).await, b"{}");

        auth.oauth = None;
        let stream = bound_to_oauth_access(&harness.state, &auth, endless_event_stream());
        let read = tokio::time::timeout(
            Duration::from_millis(200),
            to_bytes(stream.into_body(), usize::MAX),
        )
        .await;
        assert!(read.is_err(), "an API key stream stays open");
    }
}
