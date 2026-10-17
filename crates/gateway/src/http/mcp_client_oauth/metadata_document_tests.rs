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
                cache_control: cache_control.map(ToOwned::to_owned),
                age_seconds: None,
                body: serde_json::to_vec(document).unwrap(),
            },
        );
    }

    fn respond(&self, url: &str, status: u16) {
        self.documents.lock().unwrap().insert(
            url.to_string(),
            FetchedDocument {
                status,
                cache_control: Some("no-store".to_string()),
                age_seconds: None,
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
                cache_control: None,
                age_seconds: None,
                body: Vec::new(),
            }))
    }
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
async fn rate_limited_documents_fall_back_to_the_stored_client() {
    let (harness, fixture) = harness_with_fixture(&[]).await;
    fixture.publish(CLIENT_ID, &editor_document(), Some("no-store"));
    harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    // The host now rate limits; the stored copy carries on.
    fixture.respond(CLIENT_ID, 429);
    harness
        .start(&harness.authorize_query(CLIENT_ID, &[]))
        .await;
    // A missing document is not passing, so it refuses the client.
    fixture.respond(CLIENT_ID, 404);
    let (status, page) = body_text(&harness, &harness.authorize_query(CLIENT_ID, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(page.contains("HTTP 404"), "{page}");
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
