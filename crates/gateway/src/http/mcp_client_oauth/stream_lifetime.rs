//! Ends OAuth callers' MCP event streams when their access does.
//!
//! An access token is checked once, when a request arrives. A response that
//! streams for hours would otherwise outlive the token, a revoked grant, or a
//! sign-out, so the stream is closed at the token's expiry and as soon as a
//! periodic recheck finds the grant no longer live.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Response, header::CONTENT_TYPE},
};
use futures_util::StreamExt;
use gateway_core::{AuthenticatedApiKey, GatewayOauthRepository, GatewayOauthTokenKind};
use time::OffsetDateTime;

use crate::http::state::AppState;

/// How often an open stream rechecks its grant, approving session, and user.
const RECHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Bounds an event-stream `response` for an OAuth caller by its access.
/// API-key callers and other responses are returned unchanged.
pub(crate) fn bound_to_oauth_access(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    response: Response<Body>,
) -> Response<Body> {
    let Some(oauth) = auth.oauth.as_ref() else {
        return response;
    };
    if !is_event_stream(&response) {
        return response;
    }
    bound_body(
        response,
        access_ended(
            state.store.clone(),
            oauth.token_hash.clone(),
            oauth.expires_at,
            RECHECK_INTERVAL,
        ),
    )
}

fn is_event_stream(response: &Response<Body>) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim_start().starts_with("text/event-stream"))
}

fn bound_body(
    response: Response<Body>,
    ended: impl Future<Output = ()> + Send + 'static,
) -> Response<Body> {
    let (parts, body) = response.into_parts();
    let body = body.into_data_stream().take_until(Box::pin(ended));
    Response::from_parts(parts, Body::from_stream(body))
}

/// Resolves when the access token expires, or when a recheck finds it or its
/// grant no longer usable. A store error keeps the stream open until the next
/// recheck; the expiry still applies.
async fn access_ended<S>(
    store: Arc<S>,
    token_hash: String,
    expires_at: OffsetDateTime,
    every: Duration,
) where
    S: GatewayOauthRepository + ?Sized,
{
    let remaining =
        Duration::try_from(expires_at - OffsetDateTime::now_utc()).unwrap_or(Duration::ZERO);
    let expired = tokio::time::sleep(remaining);
    tokio::pin!(expired);
    loop {
        tokio::select! {
            () = &mut expired => return,
            () = tokio::time::sleep(every) => {
                let now = OffsetDateTime::now_utc();
                match store.get_gateway_oauth_token_context(&token_hash).await {
                    Ok(Some(context))
                        if context.token.kind == GatewayOauthTokenKind::Access
                            && context.grant_is_live(now) => {}
                    Ok(_) => return,
                    Err(error) => {
                        tracing::warn!(error = %error, "MCP OAuth stream recheck failed");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode};
    use gateway_core::McpOauthResource;
    use gateway_service::McpClientOauthService;

    use super::super::flow_tests::Harness;
    use super::*;

    fn endless_event_stream() -> Response<Body> {
        let chunks = futures_util::stream::once(async {
            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"data: hello\n\n"))
        })
        .chain(futures_util::stream::pending());
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream")
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
    }

    #[tokio::test]
    async fn streams_end_once_a_recheck_finds_the_grant_revoked() {
        let harness = Harness::new().await;
        let auth = oauth_caller(&harness).await;
        let oauth = auth.oauth.clone().unwrap();
        let response = bound_body(
            endless_event_stream(),
            access_ended(
                harness.state.store.clone(),
                oauth.token_hash,
                oauth.expires_at,
                Duration::from_millis(20),
            ),
        );
        harness
            .state
            .store
            .revoke_gateway_oauth_grant(oauth.grant_id, "user_revoked", OffsetDateTime::now_utc())
            .await
            .unwrap();
        assert_eq!(read_body(response).await, b"data: hello\n\n");
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
