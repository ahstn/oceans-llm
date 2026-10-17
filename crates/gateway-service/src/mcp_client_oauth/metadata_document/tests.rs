use serde_json::json;

use super::*;

const CLIENT_ID: &str = "https://editor.example.com/oauth/client.json";

fn document(overrides: Value) -> Vec<u8> {
    let mut document = json!({
        "client_id": CLIENT_ID,
        "client_name": "Editor",
        "client_uri": "https://editor.example.com",
        "redirect_uris": ["http://127.0.0.1/callback", "https://editor.example.com/cb"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    for (key, value) in overrides.as_object().expect("overrides object") {
        if value.is_null() {
            document.as_object_mut().unwrap().remove(key);
        } else {
            document[key] = value.clone();
        }
    }
    serde_json::to_vec(&document).unwrap()
}

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_790_000_000).unwrap()
}

fn rejection(overrides: Value) -> String {
    validate_document(CLIENT_ID, &document(overrides), now())
        .expect_err("document must be rejected")
        .description
}

#[test]
fn a_valid_document_becomes_a_public_client() {
    let client = validate_document(CLIENT_ID, &document(json!({})), now()).expect("valid");
    assert_eq!(client.client_id, CLIENT_ID);
    assert_eq!(
        client.registration_kind,
        METADATA_DOCUMENT_REGISTRATION_KIND
    );
    assert_eq!(client.client_name.as_deref(), Some("Editor"));
    assert_eq!(client.redirect_uris.len(), 2);
    assert_eq!(
        client.token_endpoint_auth_method,
        GatewayOauthClientAuthMethod::None
    );
    assert_eq!(client.client_secret_hash, None);
    assert_eq!(
        metadata_document_host(&client).as_deref(),
        Some("editor.example.com")
    );

    // An omitted method means none for these clients, not RFC 7591's
    // client_secret_basic default.
    let client = validate_document(
        CLIENT_ID,
        &document(json!({"token_endpoint_auth_method": null, "grant_types": null})),
        now(),
    )
    .expect("defaults");
    assert_eq!(client.grant_types, vec!["authorization_code"]);
}

#[test]
fn documents_must_name_themselves_and_their_redirects() {
    assert!(
        rejection(json!({"client_id": "https://other.example.com/oauth/client.json"}))
            .contains("does not match")
    );
    // Exact match: no normalization of case or trailing slashes.
    assert!(
        rejection(json!({"client_id": "https://Editor.example.com/oauth/client.json"}))
            .contains("does not match")
    );
    assert!(rejection(json!({"client_id": null})).contains("does not match"));
    assert!(rejection(json!({"client_name": null})).contains("client_name is required"));
    assert!(rejection(json!({"client_name": "  "})).contains("client_name is required"));
    assert!(rejection(json!({"redirect_uris": []})).contains("redirect_uris is required"));
    assert!(rejection(json!({"redirect_uris": null})).contains("redirect_uris is required"));
}

#[test]
fn documents_follow_the_registration_rules() {
    for redirect in [
        "http://editor.example.com/cb",
        "https://editor.example.com/cb#frag",
        "editor://callback",
    ] {
        assert!(
            rejection(json!({"redirect_uris": [redirect]})).contains("redirect URI"),
            "{redirect}"
        );
    }
    assert!(
        rejection(json!({"token_endpoint_auth_method": "client_secret_basic"}))
            .contains("must be none")
    );
    assert!(rejection(json!({"client_secret": "shh"})).contains("client secret"));
    assert!(rejection(json!({"grant_types": ["client_credentials"]})).contains("not supported"));
    assert!(rejection(json!({"response_types": ["token"]})).contains("response_types"));
    assert!(rejection(json!({"logo_uri": "http://editor.example.com/logo.png"})).contains("https"));
    assert!(
        validate_document(CLIENT_ID, b"[1, 2]", now())
            .unwrap_err()
            .description
            .contains("JSON object")
    );
}

#[test]
fn client_ids_follow_the_draft_rules_as_sent() {
    assert!(parse_client_id_url(CLIENT_ID).is_ok());
    assert!(parse_client_id_url("https://editor.example.com/client?v=2").is_ok());
    // Forms the URL parser rewrites are valid, distinct client IDs.
    for raw in [
        "https://editor.example.com:443/client",
        "https://EDITOR.example.com/client",
        "https://editor.example.com:8443/client",
    ] {
        assert!(parse_client_id_url(raw).is_ok(), "{raw}");
    }
    for (raw, reason) in [
        ("https://editor.example.com", "path"),
        ("https://editor.example.com/", "path"),
        ("http://editor.example.com/client", "https"),
        ("https:editor.example.com/client", "https"),
        ("HTTPS://editor.example.com/client", "https"),
        ("https://user@editor.example.com/client", "user information"),
        ("https://editor.example.com/client#x", "fragment"),
        ("https://editor.example.com/a/../client", "dot segments"),
        ("https://editor.example.com/./client", "dot segments"),
        ("https://editor.example.com/a/%2E%2e/client", "dot segments"),
        ("https://editor.example.com/a/.%2e/client", "dot segments"),
        ("https://editor.example.com/a\\..\\client", "backslashes"),
        ("https://editor.example.com/a b", "whitespace"),
    ] {
        let error = parse_client_id_url(raw).unwrap_err();
        assert_eq!(error.error, "invalid_client", "{raw}");
        assert!(
            error.description.contains(reason),
            "{raw}: {}",
            error.description
        );
    }
    let long = format!("https://editor.example.com/{}", "a".repeat(2100));
    assert!(parse_client_id_url(&long).is_err());
    assert!(is_metadata_document_client_id(CLIENT_ID));
    assert!(!is_metadata_document_client_id(
        "3f0b1c2a-7d4e-4b8a-9c1d-2e3f4a5b6c7d"
    ));
}

#[test]
fn cache_headers_set_freshness_within_bounds() {
    assert_eq!(cache_ttl(None, None), DEFAULT_CACHE_TTL);
    assert_eq!(cache_ttl(Some("public"), None), DEFAULT_CACHE_TTL);
    assert_eq!(
        cache_ttl(Some("public, max-age=120"), None),
        Duration::seconds(120)
    );
    assert_eq!(
        cache_ttl(Some("Max-Age=\"60\""), None),
        Duration::seconds(60)
    );
    assert_eq!(cache_ttl(Some("max-age=31536000"), None), MAX_CACHE_TTL);
    assert_eq!(cache_ttl(Some("max-age=0"), None), Duration::ZERO);
    assert_eq!(
        cache_ttl(Some("max-age=600, no-cache"), None),
        Duration::ZERO
    );
    assert_eq!(cache_ttl(Some("no-store"), None), Duration::ZERO);
    // Invalid or conflicting freshness is treated as stale.
    assert_eq!(cache_ttl(Some("max-age=nonsense"), None), Duration::ZERO);
    assert_eq!(cache_ttl(Some("max-age=+86400"), None), Duration::ZERO);
    assert_eq!(cache_ttl(Some("max-age=-1"), None), Duration::ZERO);
    assert_eq!(
        cache_ttl(Some("max-age=\"\"86400\"\""), None),
        Duration::ZERO
    );
    assert_eq!(cache_ttl(Some("max-age="), None), Duration::ZERO);
    assert_eq!(
        cache_ttl(Some("max-age=99999999999999999999"), None),
        MAX_CACHE_TTL
    );
    assert_eq!(
        cache_ttl(Some("max-age=0, max-age=86400"), None),
        Duration::ZERO
    );
    // A cache in between already used part of the lifetime.
    assert_eq!(
        cache_ttl(Some("max-age=86400"), Some(86_390)),
        Duration::seconds(10)
    );
    assert_eq!(cache_ttl(Some("max-age=60"), Some(600)), Duration::ZERO);
    assert_eq!(
        cache_ttl(Some("max-age=60"), Some(-5)),
        Duration::seconds(60)
    );
}

struct Unreachable;

#[async_trait]
impl ClientMetadataDocumentSource for Unreachable {
    async fn fetch(&self, _url: &Url) -> Result<FetchedDocument, OutboundFetchError> {
        panic!("the allowlist must stop the fetch");
    }
}

#[test]
fn the_domain_allowlist_matches_hosts_and_their_subdomains() {
    let documents = ClientMetadataDocuments::with_source(
        vec!["Example.com.".to_string(), "tools.dev".to_string()],
        Arc::new(Unreachable),
    )
    .expect("documents");
    assert_eq!(documents.allowed_domains(), ["example.com", "tools.dev"]);
    let allowed = |raw: &str| documents.host_is_allowed(&Url::parse(raw).unwrap());
    assert!(allowed("https://example.com/client"));
    assert!(allowed("https://editor.example.com/client"));
    assert!(allowed("https://a.b.tools.dev/client"));
    assert!(!allowed("https://badexample.com/client"));
    assert!(!allowed("https://example.com.evil.net/client"));
    assert!(!allowed("https://93.184.215.14/client"));

    for invalid in [
        "https://example.com",
        "*.example.com",
        "example.com/x",
        "",
        "-a.com",
    ] {
        assert!(
            ClientMetadataDocuments::with_source(vec![invalid.to_string()], Arc::new(Unreachable))
                .is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn must_revalidate_forbids_stale_reuse() {
    assert!(must_revalidate(Some("max-age=60, must-revalidate")));
    assert!(must_revalidate(Some("Proxy-Revalidate")));
    assert!(!must_revalidate(Some("max-age=60")));
    assert!(!must_revalidate(None));
}

#[test]
fn in_flight_entries_leave_with_their_last_caller() {
    let map = Mutex::new(HashMap::new());
    let first = InFlight::join(&map, CLIENT_ID);
    let second = InFlight::join(&map, CLIENT_ID);
    assert!(
        Arc::ptr_eq(&first.cell, &second.cell),
        "callers share a refresh"
    );
    // A cancelled waiter leaves the shared refresh in place for the other.
    drop(first);
    let third = InFlight::join(&map, CLIENT_ID);
    assert!(
        Arc::ptr_eq(&second.cell, &third.cell),
        "no second refresh starts"
    );
    // The last caller out removes the entry.
    drop(second);
    drop(third);
    assert!(map.lock().unwrap().is_empty());
}

#[tokio::test]
async fn every_cache_control_line_counts() {
    use axum::{Router, http::header, routing::get};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let router = Router::new().route(
        "/client.json",
        get(|| async {
            // Appended, so the response carries two separate field lines.
            let mut headers = axum::http::HeaderMap::new();
            headers.append(header::CACHE_CONTROL, "max-age=86400".parse().unwrap());
            headers.append(header::CACHE_CONTROL, "no-store".parse().unwrap());
            headers.append(header::AGE, "5".parse().unwrap());
            headers.append(
                header::DATE,
                "Sun, 06 Nov 1994 08:49:37 GMT".parse().unwrap(),
            );
            (headers, "{}")
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });
    let fetcher = OutboundFetcher::new(OutboundAddressScope::Loopback);
    let fetched = fetcher
        .fetch(&Url::parse(&format!("http://{address}/client.json")).unwrap())
        .await
        .expect("fetch");
    assert_eq!(
        fetched.cache_control.as_deref(),
        Some("max-age=86400, no-store")
    );
    assert_eq!(fetched.age_seconds, Some(5));
    assert_eq!(
        fetched.date,
        Some(OffsetDateTime::from_unix_timestamp(784_111_777).unwrap())
    );
    assert_eq!(
        cache_ttl(fetched.cache_control.as_deref(), fetched.age_seconds),
        Duration::ZERO
    );
}
