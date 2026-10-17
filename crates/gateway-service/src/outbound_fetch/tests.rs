use std::time::Duration;

use axum::{Router, routing::get};
use tokio::net::TcpListener;

use super::*;

fn address(raw: &str) -> IpAddr {
    raw.parse().expect("IP address")
}

fn url(raw: &str) -> Url {
    Url::parse(raw).expect("URL")
}

async fn serve(router: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let address = listener.local_addr().expect("mock address");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve mock");
    });
    address
}

#[test]
fn addresses_are_classified_by_range() {
    for public in [
        "93.184.215.14",
        "1.1.1.1",
        "2606:4700:4700::1111",
        "172.32.0.1",
    ] {
        assert_eq!(classify(address(public)), AddressClass::Public, "{public}");
    }
    for private in [
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "100.64.0.1",
        "fd12:3456::1",
    ] {
        assert_eq!(
            classify(address(private)),
            AddressClass::PrivateNetwork,
            "{private}"
        );
    }
    for loopback in ["127.0.0.1", "127.8.9.10", "::1", "::ffff:127.0.0.1"] {
        assert_eq!(
            classify(address(loopback)),
            AddressClass::Loopback,
            "{loopback}"
        );
    }
    for blocked in [
        "169.254.169.254",
        "169.254.0.1",
        "100.100.100.200",
        "168.63.129.16",
        "fd00:ec2::254",
        "0.0.0.0",
        "255.255.255.255",
        "224.0.0.1",
        "192.0.2.10",
        "::",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        // Mapped, NAT64, and 6to4 forms of the metadata address.
        "::ffff:169.254.169.254",
        "64:ff9b::a9fe:a9fe",
        "2002:a9fe:a9fe::1",
    ] {
        assert_eq!(
            classify(address(blocked)),
            AddressClass::Blocked,
            "{blocked}"
        );
    }
}

#[test]
fn each_scope_widens_the_one_before() {
    let private = address("10.0.0.5");
    let loopback = address("127.0.0.1");
    let metadata = address("169.254.169.254");
    assert!(!OutboundAddressScope::Public.allows(private));
    assert!(OutboundAddressScope::PrivateNetwork.allows(private));
    assert!(!OutboundAddressScope::PrivateNetwork.allows(loopback));
    assert!(OutboundAddressScope::Loopback.allows(loopback));
    for scope in [
        OutboundAddressScope::Public,
        OutboundAddressScope::PrivateNetwork,
        OutboundAddressScope::Loopback,
    ] {
        assert!(!scope.allows(metadata), "{scope:?}");
    }
}

#[test]
fn urls_are_checked_before_connecting() {
    let public = OutboundFetcher::new(OutboundAddressScope::Public);
    assert!(
        public
            .check_url(&url("https://client.example.com/oauth.json"))
            .is_ok()
    );
    for (raw, expected) in [
        ("http://client.example.com/oauth.json", "https"),
        ("http://127.0.0.1/oauth.json", "https"),
        ("https://user:pw@client.example.com/", "user information"),
        ("file:///etc/passwd", "no host"),
    ] {
        let error = public.check_url(&url(raw)).unwrap_err().to_string();
        assert!(error.contains(expected), "{raw}: {error}");
    }
    for raw in [
        "https://127.0.0.1/oauth.json",
        "https://10.0.0.1/oauth.json",
        "https://169.254.169.254/latest/meta-data/",
        "https://[::1]/oauth.json",
        "https://[::ffff:127.0.0.1]/oauth.json",
    ] {
        assert!(
            matches!(
                public.check_url(&url(raw)),
                Err(OutboundFetchError::BlockedAddress { .. })
            ),
            "{raw}"
        );
    }

    let loopback = OutboundFetcher::new(OutboundAddressScope::Loopback);
    assert!(
        loopback
            .check_url(&url("http://127.0.0.1:8080/mcp"))
            .is_ok()
    );
    assert!(
        loopback
            .check_url(&url("http://localhost:8080/mcp"))
            .is_ok()
    );
    assert!(loopback.check_url(&url("http://10.0.0.1/mcp")).is_err());
    assert!(matches!(
        loopback.check_url(&url("https://169.254.169.254/")),
        Err(OutboundFetchError::BlockedAddress { .. })
    ));
}

#[tokio::test]
async fn names_resolving_to_blocked_addresses_are_refused_at_connect_time() {
    let mock = serve(Router::new().route("/doc", get(|| async { "{}" }))).await;
    // `localhost` is a name, so it reaches the resolver rather than the
    // literal check, and resolves to loopback.
    let target = url(&format!("https://localhost:{}/doc", mock.port()));
    let public = OutboundFetcher::new(OutboundAddressScope::Public);
    assert!(public.check_url(&target).is_ok());
    let error = public.get(&target, "application/json").await.unwrap_err();
    assert_eq!(
        error,
        OutboundFetchError::BlockedAddress {
            host: "localhost".to_string()
        }
    );
    assert!(!error.is_transient());
}

#[tokio::test]
async fn loopback_scope_fetches_and_never_follows_redirects() {
    let mock = serve(
        Router::new()
            .route("/doc", get(|| async { "hello" }))
            .route(
                "/moved",
                get(|| async {
                    (
                        axum::http::StatusCode::FOUND,
                        [(axum::http::header::LOCATION, "http://169.254.169.254/")],
                    )
                }),
            ),
    )
    .await;
    let fetcher = OutboundFetcher::new(OutboundAddressScope::Loopback);
    let response = fetcher
        .get(&url(&format!("http://{mock}/doc")), "text/plain")
        .await
        .expect("fetch");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.body().await.expect("body"), b"hello");

    let response = fetcher
        .get(&url(&format!("http://{mock}/moved")), "text/plain")
        .await
        .expect("fetch");
    assert_eq!(response.status(), StatusCode::FOUND);
}

#[tokio::test]
async fn bodies_past_the_limit_are_refused() {
    let mock = serve(Router::new().route("/big", get(|| async { "x".repeat(2048) }))).await;
    let fetcher =
        OutboundFetcher::with_limits(OutboundAddressScope::Loopback, Duration::from_secs(5), 1024);
    let response = fetcher
        .get(&url(&format!("http://{mock}/big")), "text/plain")
        .await
        .expect("fetch");
    assert_eq!(response.body().await, Err(OutboundFetchError::TooLarge));
}

#[tokio::test]
async fn slow_servers_time_out() {
    let mock = serve(Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        }),
    ))
    .await;
    let fetcher = OutboundFetcher::with_limits(
        OutboundAddressScope::Loopback,
        Duration::from_millis(200),
        1024,
    );
    let error = fetcher
        .get(&url(&format!("http://{mock}/slow")), "text/plain")
        .await
        .unwrap_err();
    assert!(error.is_transient(), "{error:?}");
}

#[tokio::test]
async fn trusted_urls_set_the_scope_by_their_own_address() {
    assert_eq!(
        OutboundAddressScope::of_trusted_url(&url("http://127.0.0.1:9000/mcp")).await,
        OutboundAddressScope::Loopback
    );
    assert_eq!(
        OutboundAddressScope::of_trusted_url(&url("https://localhost/mcp")).await,
        OutboundAddressScope::Loopback
    );
    assert_eq!(
        OutboundAddressScope::of_trusted_url(&url("https://10.2.3.4/mcp")).await,
        OutboundAddressScope::PrivateNetwork
    );
    assert_eq!(
        OutboundAddressScope::of_trusted_url(&url("https://93.184.215.14/mcp")).await,
        OutboundAddressScope::Public
    );
    // A metadata address grants nothing beyond public.
    assert_eq!(
        OutboundAddressScope::of_trusted_url(&url("https://169.254.169.254/")).await,
        OutboundAddressScope::Public
    );
}
