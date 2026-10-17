//! Client ID Metadata Document clients, and CORS on the public endpoints,
//! through the real router and store. Documents come from a fixture, except
//! where a test checks that the network fetcher refuses internal addresses.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use gateway_core::GatewayOauthRepository;
use gateway_service::{
    ClientMetadataDocumentSource, ClientMetadataDocuments, FetchedDocument, McpClientOauthSettings,
    outbound_fetch::OutboundFetchError,
};
use serde_json::{Value, json};
use url::Url;

use super::{
    flow_tests::Harness,
    tests::{oauth_settings, send_with_headers},
};

const CLIENT_ID: &str = "https://editor.example.com/oauth/client.json";

#[derive(Default)]
struct Fixture {
    documents: Mutex<HashMap<String, FetchedDocument>>,
    fetches: AtomicUsize,
    /// Holds each fetch open this long, so concurrent requests overlap.
    delay_ms: AtomicUsize,
}

impl Fixture {
    fn publish(&self, url: &str, document: &Value, cache_control: Option<&str>) {
        self.documents.lock().unwrap().insert(
            url.to_string(),
            FetchedDocument {
                status: 200,
                headers: cache_headers(cache_control),
                body: serde_json::to_vec(document).unwrap(),
            },
        );
    }

    fn respond(&self, url: &str, status: u16) {
        self.documents.lock().unwrap().insert(
            url.to_string(),
            FetchedDocument {
                status,
                headers: cache_headers(Some("no-store")),
                body: Vec::new(),
            },
        );
    }

    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ClientMetadataDocumentSource for Fixture {
    async fn fetch(&self, url: &Url) -> Result<FetchedDocument, OutboundFetchError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        let delay = self.delay_ms.load(Ordering::SeqCst) as u64;
        if delay > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }
        Ok(self
            .documents
            .lock()
            .unwrap()
            .get(url.as_str())
            .cloned()
            .unwrap_or(FetchedDocument {
                status: 404,
                headers: cache_headers(None),
                body: Vec::new(),
            }))
    }
}

fn cache_headers(cache_control: Option<&str>) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    if let Some(value) = cache_control {
        headers.insert(header::CACHE_CONTROL, value.parse().unwrap());
    }
    headers
}

fn editor_document() -> Value {
    json!({
        "client_id": CLIENT_ID,
        "client_name": "Web Editor",
        "client_uri": "https://editor.example.com",
        "redirect_uris": ["http://127.0.0.1/callback", "https://editor.example.com/cb"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    })
}

fn settings_with(documents: ClientMetadataDocuments) -> McpClientOauthSettings {
    oauth_settings().with_client_id_metadata_documents(documents)
}

async fn harness_with_fixture(allowed_domains: &[&str]) -> (Harness, Arc<Fixture>) {
    let fixture = Arc::new(Fixture::default());
    let documents = ClientMetadataDocuments::with_source(
        allowed_domains.iter().map(ToString::to_string).collect(),
        fixture.clone(),
    )
    .expect("documents");
    (
        Harness::with_settings(settings_with(documents)).await,
        fixture,
    )
}

async fn body_text(harness: &Harness, query: &str) -> (StatusCode, String) {
    let (status, _, body) = send_with_headers(
        &harness.router,
        Request::builder()
            .uri(format!("/oauth/authorize?{query}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    (status, body.as_str().unwrap_or_default().to_string())
}

#[tokio::test]
async fn a_valid_document_signs_in_without_registration() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), None);

    let (_, _, metadata) = send_with_headers(
        &harness.router,
        Request::builder()
            .uri("/.well-known/oauth-authorization-server")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(metadata["client_id_metadata_document_supported"], true);

    let request_id = harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    let (status, view) = harness.consent(&request_id).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["client_id"], CLIENT_ID);
    assert_eq!(view["client_name"], "Web Editor");
    assert_eq!(view["client_metadata_host"], "editor.example.com");

    let code = harness.approve(&request_id).await;
    let approved = harness.audit_entries("mcp_client_grant.authorized").await;
    assert_eq!(
        approved[0].after.as_ref().unwrap()["client_metadata_host"],
        "editor.example.com"
    );
    let (status, token) = harness.exchange_code(CLIENT_ID, &code).await;
    assert_eq!(status, StatusCode::OK, "{token}");
    let refresh_token = token["refresh_token"].as_str().expect("refresh token");

    let (status, _, refreshed) = harness
        .token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", refresh_token),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");

    let stored = harness
        .state
        .store
        .get_gateway_oauth_client(CLIENT_ID)
        .await
        .unwrap()
        .expect("stored client");
    assert_eq!(stored.registration_kind, "metadata_document");
    // Within the default freshness, one fetch served authorize, the code
    // exchange, and the refresh.
    assert_eq!(fixture.fetches(), 1);
}

#[tokio::test]
async fn a_document_that_names_another_client_is_refused() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    let mut document = editor_document();
    document["client_id"] = json!("https://other.example.com/oauth/client.json");
    fixture.publish(CLIENT_ID, &document, None);

    let (status, page) = body_text(&harness, &harness.authorize_query(CLIENT_ID, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("does not match its URL"), "{page}");
    assert!(page.contains("invalid_client"), "{page}");
    assert_eq!(
        harness
            .state
            .store
            .get_gateway_oauth_client(CLIENT_ID)
            .await
            .unwrap(),
        None
    );

    let missing = "https://editor.example.com/oauth/missing.json";
    let (status, page) = body_text(&harness, &harness.authorize_query(missing, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("HTTP 404"), "{page}");
}

#[tokio::test]
async fn redirect_uris_must_be_listed_in_the_document() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    let mut document = editor_document();
    document["redirect_uris"] = json!(["https://editor.example.com/cb"]);
    fixture.publish(CLIENT_ID, &document, Some("no-store"));

    // The request's loopback redirect is not in the document, so the
    // gateway shows an error instead of redirecting anywhere.
    let (status, page) = body_text(&harness, &harness.authorize_query(CLIENT_ID, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("redirect_uri is not registered"), "{page}");

    // A document that drops a redirect after consent stops the code.
    fixture.publish(CLIENT_ID, &editor_document(), Some("no-store"));
    let request_id = harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    let code = harness.approve(&request_id).await;
    fixture.publish(CLIENT_ID, &document, Some("no-store"));
    let (status, body) = harness.exchange_code(CLIENT_ID, &code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_grant");

    // Documents with an http redirect off loopback break the registration rules.
    let mut insecure = editor_document();
    insecure["redirect_uris"] = json!(["http://editor.example.com/cb"]);
    fixture.publish(CLIENT_ID, &insecure, Some("no-store"));
    let (status, page) = body_text(&harness, &harness.authorize_query(CLIENT_ID, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("must use https"), "{page}");
}

#[tokio::test]
async fn documents_on_internal_addresses_are_never_fetched() {
    // The network fetcher, not the fixture.
    let documents = ClientMetadataDocuments::new(Vec::new()).expect("documents");
    let harness = Harness::with_settings(settings_with(documents)).await;
    for client_id in [
        "https://127.0.0.1/client.json",
        "https://10.0.0.8/client.json",
        "https://169.254.169.254/latest/meta-data/client.json",
        "https://[::1]/client.json",
        // A name: refused by the resolver once it resolves to loopback.
        "https://localhost/client.json",
    ] {
        let (status, page) = body_text(&harness, &harness.authorize_query(client_id, &[])).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{client_id}: {page}");
        assert!(
            page.contains("address that is not allowed"),
            "{client_id}: {page}"
        );
    }
}

#[tokio::test]
async fn documents_are_cached_as_their_headers_allow() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), Some("public, max-age=600"));
    for _ in 0..3 {
        harness
            .start(&harness.authorize_query(CLIENT_ID, &[]))
            .await;
    }
    assert_eq!(fixture.fetches(), 1);

    let uncached = "https://editor.example.com/oauth/fresh.json";
    let mut document = editor_document();
    document["client_id"] = json!(uncached);
    fixture.publish(uncached, &document, Some("no-store"));
    harness.start(&harness.authorize_query(uncached, &[])).await;
    harness.start(&harness.authorize_query(uncached, &[])).await;
    assert_eq!(fixture.fetches(), 3);

    // A change takes effect on the next fetch of an uncached document.
    document["client_name"] = json!("Renamed Editor");
    fixture.publish(uncached, &document, Some("no-store"));
    let request_id = harness.start(&harness.authorize_query(uncached, &[])).await;
    let (_, view) = harness.consent(&request_id).await;
    assert_eq!(view["client_name"], "Renamed Editor");
}

#[tokio::test]
async fn concurrent_requests_share_one_fetch() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    // Uncacheable, so only coalescing keeps this to one fetch.
    fixture.publish(CLIENT_ID, &editor_document(), Some("no-store"));
    fixture.delay_ms.store(200, Ordering::SeqCst);
    let query = harness.authorize_query(CLIENT_ID, &[]);
    let starts = (0..5).map(|_| harness.authorize(&query));
    for (status, _) in futures_util::future::join_all(starts).await {
        assert_eq!(status, StatusCode::SEE_OTHER);
    }
    assert_eq!(fixture.fetches(), 1);
}

#[tokio::test]
async fn document_fetches_are_capped() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    // Slow, distinct documents; each holds a fetch slot while it waits.
    fixture.delay_ms.store(300, Ordering::SeqCst);
    let queries = (0..40)
        .map(|index| {
            harness.authorize_query(
                &format!("https://editor.example.com/clients/{index}.json"),
                &[],
            )
        })
        .collect::<Vec<_>>();
    let pages =
        futures_util::future::join_all(queries.iter().map(|query| body_text(&harness, query)))
            .await;
    let unavailable = pages
        .iter()
        .filter(|(status, page)| {
            *status == StatusCode::SERVICE_UNAVAILABLE && page.contains("temporarily_unavailable")
        })
        .count();
    assert!(unavailable >= 8, "{unavailable} requests were turned away");
    assert!(fixture.fetches() <= 32, "{} fetches ran", fixture.fetches());
}

#[tokio::test]
async fn passing_failures_reuse_only_a_cacheable_copy() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), Some("max-age=1"));
    harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    // Past its freshness, the host rate limits; a burst shares one fetch and
    // the cached copy stands in.
    fixture.respond(CLIENT_ID, 429);
    fixture.delay_ms.store(200, Ordering::SeqCst);
    let fetches_before = fixture.fetches();
    let query = harness.authorize_query(CLIENT_ID, &[]);
    for (status, _) in
        futures_util::future::join_all((0..5).map(|_| harness.authorize(&query))).await
    {
        assert_eq!(status, StatusCode::SEE_OTHER);
    }
    assert_eq!(fixture.fetches(), fetches_before + 1);
    fixture.delay_ms.store(0, Ordering::SeqCst);
    // The stand-in is then served for a while without trying the host.
    fixture.respond(CLIENT_ID, 408);
    harness.start(&query).await;
    assert_eq!(fixture.fetches(), fetches_before + 1);

    // The stand-in is written back, so cleanup sees it in use.
    let stored = harness
        .state
        .store
        .get_gateway_oauth_client(CLIENT_ID)
        .await
        .unwrap()
        .expect("stored client");
    assert!(stored.updated_at > stored.created_at, "{stored:?}");

    // A must-revalidate document is not reused once stale either.
    let strict = "https://editor.example.com/oauth/strict.json";
    let mut strict_document = editor_document();
    strict_document["client_id"] = json!(strict);
    fixture.publish(strict, &strict_document, Some("max-age=1, must-revalidate"));
    harness.start(&harness.authorize_query(strict, &[])).await;
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    fixture.respond(strict, 503);
    let (status, page) = body_text(&harness, &harness.authorize_query(strict, &[])).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{page}");

    // A no-store document is never reused, so a passing failure is a
    // retryable error rather than a stand-in.
    let uncached = "https://editor.example.com/oauth/no-store.json";
    let mut document = editor_document();
    document["client_id"] = json!(uncached);
    fixture.publish(uncached, &document, Some("no-store"));
    harness.start(&harness.authorize_query(uncached, &[])).await;
    fixture.respond(uncached, 429);
    let (status, page) = body_text(&harness, &harness.authorize_query(uncached, &[])).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{page}");
    assert!(page.contains("temporarily_unavailable"), "{page}");
}

#[tokio::test]
async fn withdrawn_documents_are_refused_without_refetching_or_standing_in() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), Some("max-age=1"));
    harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    // The publisher withdraws the document.
    fixture.respond(CLIENT_ID, 404);
    let query = harness.authorize_query(CLIENT_ID, &[]);
    let (status, page) = body_text(&harness, &query).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{page}");
    let fetches = fixture.fetches();
    // Repeats are refused from memory, and an outage now cannot bring the
    // withdrawn copy back.
    fixture.respond(CLIENT_ID, 503);
    for _ in 0..3 {
        let (status, page) = body_text(&harness, &query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{page}");
        assert!(page.contains("HTTP 404"), "{page}");
    }
    assert_eq!(fixture.fetches(), fetches);
}

#[tokio::test]
async fn the_domain_allowlist_stops_other_hosts_before_fetching() {
    let (harness, fixture) = harness_with_fixture(&["trusted.example"]).await;
    fixture.publish(CLIENT_ID, &editor_document(), None);
    let (status, page) = body_text(&harness, &harness.authorize_query(CLIENT_ID, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("not accepted"), "{page}");
    assert_eq!(fixture.fetches(), 0);
}

#[tokio::test]
async fn revocation_still_works_once_the_document_is_gone() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), Some("no-store"));
    let request_id = harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    let code = harness.approve(&request_id).await;
    let (_, token) = harness.exchange_code(CLIENT_ID, &code).await;
    let refresh_token = token["refresh_token"].as_str().unwrap().to_string();

    fixture.documents.lock().unwrap().clear();
    // Refreshing needs a valid document; revoking does not.
    let (status, _, body) = harness
        .token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", &refresh_token),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "invalid_client");

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", CLIENT_ID)
        .append_pair("token", &refresh_token)
        .finish();
    let (status, _, _) = send_with_headers(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/oauth/revoke")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let grant = harness
        .state
        .store
        .get_gateway_oauth_token_context(&gateway_service::mcp_client_oauth::oauth_token_hash(
            &refresh_token,
        ))
        .await
        .unwrap()
        .expect("token");
    assert!(grant.grant.revoked_at.is_some());
}

#[tokio::test]
async fn revocation_still_works_once_documents_are_turned_off() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), None);
    let request_id = harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    let code = harness.approve(&request_id).await;
    let (_, token) = harness.exchange_code(CLIENT_ID, &code).await;
    let refresh_token = token["refresh_token"].as_str().unwrap().to_string();

    // The same store, with documents turned off.
    let mut state = harness.state.clone();
    state.mcp_client_oauth = Some(std::sync::Arc::new(oauth_settings()));
    let router = super::tests::router(&state);
    let refreshed = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("refresh_token", &refresh_token)
        .finish();
    let (status, _, _) = send_with_headers(
        &router,
        Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(refreshed))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "refresh is refused");
    let revoke = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", CLIENT_ID)
        .append_pair("token", &refresh_token)
        .finish();
    let (status, _, _) = send_with_headers(
        &router,
        Request::builder()
            .method("POST")
            .uri("/oauth/revoke")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(revoke))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "revocation still works");
    let context = harness
        .state
        .store
        .get_gateway_oauth_token_context(&gateway_service::mcp_client_oauth::oauth_token_hash(
            &refresh_token,
        ))
        .await
        .unwrap()
        .expect("token");
    assert!(context.grant.revoked_at.is_some());
}

#[tokio::test]
async fn documents_are_off_when_disabled() {
    let harness = Harness::new().await;
    let (_, _, metadata) = send_with_headers(
        &harness.router,
        Request::builder()
            .uri("/.well-known/oauth-authorization-server")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(metadata["client_id_metadata_document_supported"], false);
    let (status, page) = body_text(&harness, &harness.authorize_query(CLIENT_ID, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("not registered"), "{page}");
}

#[tokio::test]
async fn public_endpoints_allow_cross_origin_calls_without_credentials() {
    let harness = Harness::new().await;
    let origin = "https://app.example.net";
    for (method, uri) in [
        ("POST", "/oauth/token"),
        ("POST", "/oauth/revoke"),
        ("POST", "/oauth/register"),
        ("GET", "/.well-known/oauth-authorization-server"),
        ("GET", "/.well-known/oauth-protected-resource/mcp"),
    ] {
        let (status, headers, _) = send_with_headers(
            &harness.router,
            Request::builder()
                .method("OPTIONS")
                .uri(uri)
                .header(header::ORIGIN, origin)
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "authorization, content-type, mcp-protocol-version",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*", "{uri}");
        assert!(
            headers[header::ACCESS_CONTROL_ALLOW_METHODS]
                .to_str()
                .unwrap()
                .contains(method),
            "{uri}"
        );
        assert!(
            !headers.contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            "{uri}"
        );
    }

    // Actual responses, errors included, carry the header too.
    let (status, headers, body) = send_with_headers(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("grant_type=authorization_code&client_id=nobody"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    // The browser-facing and cookie-authenticated endpoints stay same-origin.
    for uri in [
        format!(
            "/oauth/authorize?{}",
            harness.authorize_query("editor", &[])
        ),
        "/api/v1/oauth/authorization-requests/00000000-0000-0000-0000-000000000000".to_string(),
    ] {
        let (_, headers, _) = send_with_headers(
            &harness.router,
            Request::builder()
                .uri(&uri)
                .header(header::ORIGIN, origin)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(
            !headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn mcp_routes_let_browser_clients_read_the_challenge() {
    let harness = Harness::new().await;
    let origin = "https://app.example.net";
    for uri in ["/mcp", "/mcp/repository"] {
        let (status, headers, _) = send_with_headers(
            &harness.router,
            Request::builder()
                .method("OPTIONS")
                .uri(uri)
                .header(header::ORIGIN, origin)
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "authorization, content-type, mcp-session-id, mcp-protocol-version",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*", "{uri}");
        assert!(
            !headers.contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            "{uri}"
        );
    }

    // The unauthenticated call that starts discovery: the page can read the
    // status and the challenge.
    let (status, headers, _) = send_with_headers(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    let exposed = headers[header::ACCESS_CONTROL_EXPOSE_HEADERS]
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(exposed.contains("www-authenticate"), "{exposed}");
    assert!(exposed.contains("mcp-session-id"), "{exposed}");
    assert!(headers.contains_key(header::WWW_AUTHENTICATE));
}
