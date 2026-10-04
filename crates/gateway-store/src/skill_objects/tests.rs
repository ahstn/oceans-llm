use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::{net::TcpListener, sync::Mutex};

use super::*;

fn config(endpoint: &str) -> S3SkillStorageConfig {
    S3SkillStorageConfig {
        bucket: "skills-test".to_owned(),
        region: "us-east-1".to_owned(),
        endpoint: Some(endpoint.to_owned()),
        prefix: "archives/".to_owned(),
        force_path_style: true,
        allow_http: true,
        access_key_id: Some("test-access-key".to_owned()),
        secret_access_key: Some("test-secret-key".to_owned()),
        session_token: None,
        max_upload_bytes: 1024,
        request_timeout: Duration::from_secs(5),
    }
}

#[test]
fn rejects_invalid_storage_settings_without_exposing_credentials() {
    let mut settings = config("http://127.0.0.1:9000");
    settings.allow_http = false;
    assert!(validate_config(&settings).is_err());
    settings.allow_http = true;
    settings.endpoint = Some("https://private:secret@example.com".to_owned());
    let error = validate_config(&settings).unwrap_err().to_string();
    assert!(!error.contains("private"));
    assert!(!error.contains("secret"));
    settings.endpoint = None;
    settings.secret_access_key = None;
    assert!(validate_config(&settings).is_err());
    settings.access_key_id = None;
    assert!(validate_config(&settings).is_ok());
    settings.session_token = Some("token-without-keys".to_owned());
    assert!(validate_config(&settings).is_err());
}

#[test]
fn rejects_keys_that_escape_or_obscure_the_prefix() {
    for key in [
        "",
        "/archive.zip",
        "../archive.zip",
        "a/./b",
        "a//b",
        "a\\b",
        "a\nb",
    ] {
        assert!(validate_key(key).is_err(), "accepted invalid key");
    }
    assert!(validate_key("owner/skill/1-abcd.zip").is_ok());
    assert!(validate_key(&"a".repeat(1025)).is_err());
}

#[tokio::test]
async fn streamed_read_enforces_limit_without_trusting_content_length() {
    let body = ByteStream::from_static(b"archive");
    assert_eq!(read_bounded_body(body, 7).await.unwrap(), b"archive");
    assert!(
        read_bounded_body(ByteStream::from_static(b"archive"), 6)
            .await
            .is_err()
    );
}

#[derive(Default)]
struct S3Fixture {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}

async fn mock_s3(State(fixture): State<Arc<S3Fixture>>, request: Request) -> Response {
    assert!(request.headers().contains_key("authorization"));
    let key = request.uri().path().to_owned();
    assert!(key.starts_with("/skills-test/archives/"));
    if key.ends_with("/denied.zip") {
        return (
            StatusCode::FORBIDDEN,
            "<Error><Code>AccessDenied</Code><Message>server-secret-value</Message></Error>",
        )
            .into_response();
    }
    if key.ends_with("/slow.zip") {
        return Response::new(Body::from_stream(futures_util::stream::once(async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok::<_, std::convert::Infallible>("delayed body")
        })));
    }
    if key.ends_with("/streamed.zip") {
        return Response::new(Body::from_stream(futures_util::stream::iter([
            Ok::<_, std::convert::Infallible>("arc"),
            Ok("hive"),
        ])));
    }
    match *request.method() {
        Method::PUT => {
            assert_eq!(request.headers()["if-none-match"], "*");
            let body = to_bytes(request.into_body(), 4096).await.unwrap();
            let mut objects = fixture.objects.lock().await;
            match objects.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(body.to_vec());
                    StatusCode::OK.into_response()
                }
                Entry::Occupied(_) => (
                    StatusCode::PRECONDITION_FAILED,
                    "<Error><Code>PreconditionFailed</Code></Error>",
                )
                    .into_response(),
            }
        }
        Method::GET => {
            let objects = fixture.objects.lock().await;
            match objects.get(&key) {
                Some(bytes) => Response::new(Body::from(bytes.clone())),
                None => (
                    StatusCode::NOT_FOUND,
                    "<Error><Code>NoSuchKey</Code></Error>",
                )
                    .into_response(),
            }
        }
        Method::DELETE => {
            fixture.objects.lock().await.remove(&key);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

#[tokio::test]
async fn s3_requests_preserve_immutable_archive_and_map_missing_objects() {
    let fixture = Arc::new(S3Fixture::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().fallback(mock_s3).with_state(fixture.clone());
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let store = S3SkillObjectStore::new(config(&endpoint)).await.unwrap();

    store.put("test.zip", b"archive").await.unwrap();
    assert_eq!(store.get("test.zip", 7).await.unwrap(), b"archive");
    assert!(matches!(
        store.put("test.zip", b"replacement").await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(store.get("test.zip", 7).await.unwrap(), b"archive");
    assert!(store.put("large.zip", &[0; 1025]).await.is_err());
    assert!(store.get("../test.zip", 7).await.is_err());
    fixture
        .objects
        .lock()
        .await
        .insert("/skills-test/archives/large.zip".to_owned(), vec![0; 1025]);
    assert!(store.get("large.zip", 1024).await.is_err());
    let denied = store.get("denied.zip", 1024).await.unwrap_err().to_string();
    assert!(denied.contains("403"));
    assert!(!denied.contains("server-secret-value"));
    store.delete("test.zip").await.unwrap();
    assert!(matches!(
        store.get("test.zip", 7).await,
        Err(StoreError::NotFound(_))
    ));
    store.delete("test.zip").await.unwrap();
    let mut short_timeout = config(&endpoint);
    short_timeout.request_timeout = Duration::from_millis(100);
    let store = S3SkillObjectStore::new(short_timeout).await.unwrap();
    let timeout = store.get("slow.zip", 1024).await.unwrap_err().to_string();
    assert!(timeout.contains("timed out"));
    server.abort();
}

#[tokio::test]
async fn reduced_upload_limit_preserves_reads_with_recorded_archive_bounds() {
    let fixture = Arc::new(S3Fixture::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().fallback(mock_s3).with_state(fixture);
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let store = S3SkillObjectStore::new(config(&endpoint)).await.unwrap();
    store.put("test.zip", b"archive").await.unwrap();

    let mut reduced = config(&endpoint);
    reduced.max_upload_bytes = 1;
    let store = S3SkillObjectStore::new(reduced).await.unwrap();
    assert!(matches!(
        store.put("new.zip", b"archive").await,
        Err(StoreError::Unexpected(_))
    ));
    assert_eq!(store.get("test.zip", 7).await.unwrap(), b"archive");
    // Known Content-Length and chunked bodies must both obey the persisted bound.
    assert!(matches!(
        store.get("test.zip", 6).await,
        Err(StoreError::Unexpected(_))
    ));
    assert_eq!(store.get("streamed.zip", 7).await.unwrap(), b"archive");
    assert!(matches!(
        store.get("streamed.zip", 6).await,
        Err(StoreError::Unexpected(_))
    ));
    server.abort();
}
