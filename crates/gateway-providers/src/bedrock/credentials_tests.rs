use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::UNIX_EPOCH,
};

use super::*;
use aws_credential_types::provider::{self, future};

#[derive(Debug)]
struct SequenceProvider {
    calls: AtomicUsize,
    results: StdMutex<VecDeque<provider::Result>>,
}

impl SequenceProvider {
    fn new(results: impl IntoIterator<Item = provider::Result>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            results: StdMutex::new(results.into_iter().collect()),
        }
    }
}

impl ProvideCredentials for SequenceProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected credential refresh")
        })
    }
}

fn credentials(key: &str, expiry: Option<SystemTime>) -> Credentials {
    Credentials::new(
        key,
        "test-secret",
        Some("test-session".into()),
        expiry,
        "test",
    )
}

#[tokio::test]
async fn concurrent_callers_share_initial_load_and_expiry_refresh() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let cache = Arc::new(CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("first", Some(now + Duration::from_secs(300)))),
        Ok(credentials("second", Some(now + Duration::from_secs(600)))),
    ])));
    for (time, key, count) in [
        (now, "first", 1),
        (now + Duration::from_secs(239), "first", 1),
        (now + Duration::from_secs(240), "second", 2),
    ] {
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let cache = Arc::clone(&cache);
            tasks.spawn(async move { cache.credentials_with_clock(|| time).await.unwrap() });
        }
        while let Some(result) = tasks.join_next().await {
            assert_eq!(result.unwrap().access_key_id(), key);
        }
        assert_eq!(cache.provider.calls.load(Ordering::SeqCst), count);
    }
}

#[tokio::test]
async fn short_lived_and_undated_credentials_do_not_refresh_on_every_call() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    for (expiry, interval) in [
        (Some(now + Duration::from_secs(10)), Duration::from_secs(8)),
        (None, UNDATED_CREDENTIAL_TTL),
    ] {
        let cache = CachedCredentials::new(SequenceProvider::new([
            Ok(credentials("first", expiry)),
            Ok(credentials("second", None)),
        ]));
        for time in [now, now + interval - Duration::from_nanos(1)] {
            assert_eq!(
                cache
                    .credentials_with_clock(|| time)
                    .await
                    .unwrap()
                    .access_key_id(),
                "first"
            );
        }
        assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            cache
                .credentials_with_clock(|| now + interval)
                .await
                .unwrap()
                .access_key_id(),
            "second"
        );
        assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn rejects_expired_results_and_retries_after_provider_failures() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let cache = CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("old", Some(now + Duration::from_secs(300)))),
        Err(CredentialsError::not_loaded("mock unavailable")),
        Ok(credentials("expired", Some(now + Duration::from_secs(300)))),
        Ok(credentials("fresh", Some(now + Duration::from_secs(900)))),
    ]));
    cache.credentials_with_clock(|| now).await.unwrap();
    let later = now + Duration::from_secs(300);
    assert!(matches!(
        cache.credentials_with_clock(|| later).await,
        Err(CredentialCacheError::Resolve(_))
    ));
    assert!(matches!(
        cache
            .credentials_with_clock(|| later + FAILURE_RETRY_DELAY)
            .await,
        Err(CredentialCacheError::Expired)
    ));
    assert_eq!(
        cache
            .credentials_with_clock(|| later + FAILURE_RETRY_DELAY * 2)
            .await
            .unwrap()
            .access_key_id(),
        "fresh"
    );
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn concurrent_failed_refresh_is_shared_until_retry_delay_passes() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let cache = Arc::new(CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("old", Some(now + Duration::from_secs(300)))),
        Err(CredentialsError::not_loaded("mock STS outage")),
        Ok(credentials("recovered", None)),
    ])));
    cache.credentials_with_clock(|| now).await.unwrap();
    let later = now + Duration::from_secs(300);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let cache = Arc::clone(&cache);
        tasks.spawn(async move { cache.credentials_with_clock(|| later).await });
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(result) = tasks.join_next().await {
            assert!(matches!(
                result.unwrap(),
                Err(CredentialCacheError::Resolve(_))
            ));
        }
    })
    .await
    .expect("waiters must share failure without repeated STS calls");
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        cache
            .credentials_with_clock(|| later + FAILURE_RETRY_DELAY - Duration::from_nanos(1))
            .await,
        Err(CredentialCacheError::Resolve(_))
    ));
    assert_eq!(
        cache
            .credentials_with_clock(|| later + FAILURE_RETRY_DELAY)
            .await
            .unwrap()
            .access_key_id(),
        "recovered"
    );
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn proactive_refresh_failure_keeps_valid_credentials_and_recovers() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let cache = Arc::new(CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("old", Some(now + Duration::from_secs(300)))),
        Err(CredentialsError::not_loaded("mock STS outage")),
        Ok(credentials(
            "recovered",
            Some(now + Duration::from_secs(600)),
        )),
        Err(CredentialsError::not_loaded("mock STS outage after expiry")),
    ])));
    cache.credentials_with_clock(|| now).await.unwrap();
    let refresh = now + Duration::from_secs(240);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let cache = Arc::clone(&cache);
        tasks.spawn(async move { cache.credentials_with_clock(|| refresh).await });
    }
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap().unwrap().access_key_id(), "old");
    }
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
    for (time, expected_key, expected_calls) in [
        (refresh + Duration::from_millis(500), "old", 2),
        (refresh + FAILURE_RETRY_DELAY, "recovered", 3),
    ] {
        assert_eq!(
            cache
                .credentials_with_clock(|| time)
                .await
                .unwrap()
                .access_key_id(),
            expected_key
        );
        assert_eq!(cache.provider.calls.load(Ordering::SeqCst), expected_calls);
    }
    assert!(matches!(
        cache
            .credentials_with_clock(|| now + Duration::from_secs(600))
            .await,
        Err(CredentialCacheError::Resolve(_))
    ));
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn failed_refresh_retry_never_extends_past_credential_expiry() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let expiry = now + Duration::from_secs(300);
    let cache = CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("old", Some(expiry))),
        Err(CredentialsError::not_loaded("mock STS outage")),
        Err(CredentialsError::not_loaded("mock STS still unavailable")),
    ]));
    cache.credentials_with_clock(|| now).await.unwrap();
    for time in [
        expiry - Duration::from_millis(500),
        expiry - Duration::from_nanos(1),
    ] {
        assert_eq!(
            cache
                .credentials_with_clock(|| time)
                .await
                .unwrap()
                .access_key_id(),
            "old"
        );
        assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
    }
    for time in [expiry, expiry + Duration::from_millis(500)] {
        assert!(matches!(
            cache.credentials_with_clock(|| time).await,
            Err(CredentialCacheError::Resolve(_))
        ));
        assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn failed_refresh_checks_expiry_after_resolution_finishes() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let expiry = now + Duration::from_secs(300);
    let cache = CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("old", Some(expiry))),
        Err(CredentialsError::not_loaded("mock slow STS failure")),
    ]));
    cache.credentials_with_clock(|| now).await.unwrap();
    let clock_reads = std::cell::Cell::new(0);
    let result = cache
        .credentials_with_clock(|| {
            let read = clock_reads.get();
            clock_reads.set(read + 1);
            if read == 0 {
                expiry - Duration::from_secs(1)
            } else {
                expiry
            }
        })
        .await;
    assert!(matches!(result, Err(CredentialCacheError::Resolve(_))));
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn undated_credentials_are_not_extended_after_failed_refresh() {
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let cache = CachedCredentials::new(SequenceProvider::new([
        Ok(credentials("undated", None)),
        Err(CredentialsError::not_loaded(
            "mock credential source unavailable",
        )),
    ]));
    cache.credentials_with_clock(|| now).await.unwrap();
    assert!(matches!(
        cache
            .credentials_with_clock(|| now + UNDATED_CREDENTIAL_TTL)
            .await,
        Err(CredentialCacheError::Resolve(_))
    ));
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn sdk_assume_role_uses_one_mock_sts_call_for_repeated_and_concurrent_requests() {
    use aws_config::{BehaviorVersion, Region, sts::AssumeRoleProvider};
    use axum::{Router, routing::post};

    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = Arc::clone(&calls);
    let expiry = time::OffsetDateTime::now_utc() + time::Duration::hours(1);
    let expiry = expiry
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let app = Router::new().route("/", post(move |body: String| {
        let calls = Arc::clone(&handler_calls);
        let expiry = expiry.clone();
        async move {
            assert!(body.contains("Action=AssumeRole"));
            calls.fetch_add(1, Ordering::SeqCst);
            ([ ("content-type", "text/xml") ], format!(
                "<AssumeRoleResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\"><AssumeRoleResult><Credentials><AccessKeyId>mock-access</AccessKeyId><SecretAccessKey>mock-secret</SecretAccessKey><SessionToken>mock-session</SessionToken><Expiration>{expiry}</Expiration></Credentials><AssumedRoleUser><Arn>arn:aws:sts::123456789012:assumed-role/test/session</Arn><AssumedRoleId>test:session</AssumedRoleId></AssumedRoleUser></AssumeRoleResult><ResponseMetadata><RequestId>test</RequestId></ResponseMetadata></AssumeRoleResponse>"
            ))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    // Use aws-config's re-export to keep the mock independent of the host's
    // profile files without adding an aws-runtime dependency solely for tests.
    #[allow(deprecated)]
    let profile_files = aws_config::profile::profile_file::ProfileFiles::builder()
        .with_contents(
            aws_config::profile::profile_file::ProfileFileKind::Config,
            "",
        )
        .with_contents(
            aws_config::profile::profile_file::ProfileFileKind::Credentials,
            "",
        )
        .build();
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(endpoint)
        .credentials_provider(credentials("source", None))
        .retry_config(aws_config::retry::RetryConfig::disabled())
        .profile_files(profile_files)
        .load()
        .await;
    let source = AssumeRoleProvider::builder("arn:aws:iam::123456789012:role/test")
        .session_name("test")
        .configure(&config)
        .build()
        .await;
    let cache = Arc::new(CachedCredentials::new(source));
    for _ in 0..2 {
        assert_eq!(
            cache.credentials().await.unwrap().session_token(),
            Some("mock-session")
        );
    }
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let cache = Arc::clone(&cache);
        tasks.spawn(async move { cache.credentials().await.unwrap() });
    }
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap().access_key_id(), "mock-access");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}

#[derive(Debug, Default)]
struct InterruptedProvider {
    calls: AtomicUsize,
    started: tokio::sync::Notify,
}

impl ProvideCredentials for InterruptedProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(async {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.started.notify_one();
                std::future::pending::<()>().await;
            }
            Ok(credentials("recovered", None))
        })
    }
}

#[tokio::test]
async fn cancelling_refresh_releases_waiting_callers() {
    let cache = Arc::new(CachedCredentials::new(InterruptedProvider::default()));
    let loading_cache = Arc::clone(&cache);
    let loading = tokio::spawn(async move { loading_cache.credentials().await });
    cache.provider.started.notified().await;
    let waiting_cache = Arc::clone(&cache);
    let waiting = tokio::spawn(async move { waiting_cache.credentials().await });
    loading.abort();
    assert!(loading.await.unwrap_err().is_cancelled());
    let result = tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .expect("cancelled refresh must not hold the lock")
        .unwrap()
        .unwrap();
    assert_eq!(result.access_key_id(), "recovered");
    assert_eq!(cache.provider.calls.load(Ordering::SeqCst), 2);
}
