//! Outbound HTTP for URLs that someone other than an admin chose: OAuth
//! metadata named by an upstream MCP server, or a client's metadata document.
//!
//! A URL is only as safe as the address it resolves to, so the guard sits in
//! the DNS resolver. The client connects to exactly the addresses the resolver
//! returned after checking them, which closes the gap between checking a name
//! and connecting to it (DNS rebinding). IP-literal hosts never reach the
//! resolver, so [`OutboundFetcher::check_url`] checks those. Redirects and
//! proxies are off, since either would move the connection somewhere the guard
//! did not check. See `docs/adr/2026-10-17-ssrf-safe-outbound-fetch.md`.

use std::{
    error::Error as StdError,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use reqwest::{
    Method, StatusCode,
    dns::{Addrs, Name, Resolve, Resolving},
    header::HeaderMap,
};
use url::{Host, Url};

pub const DEFAULT_OUTBOUND_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_OUTBOUND_MAX_BODY_BYTES: usize = 256 * 1024;

/// Which addresses a fetch may connect to. Each scope allows everything the
/// one before it does.
///
/// No scope reaches link-local addresses, which hold the cloud metadata
/// services, or unspecified, multicast, and reserved ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutboundAddressScope {
    /// Globally routable addresses only.
    Public,
    /// Also private networks: RFC 1918, carrier-grade NAT, and IPv6 unique
    /// local addresses.
    PrivateNetwork,
    /// Also loopback, and plain `http` to a loopback host.
    Loopback,
}

impl OutboundAddressScope {
    /// The scope matching an admin-entered URL: a server an admin registered
    /// on a private network or on this host may point at others there, and a
    /// public one may not.
    pub async fn of_trusted_url(url: &Url) -> Self {
        let addresses = match url.host() {
            Some(Host::Ipv4(address)) => vec![IpAddr::V4(address)],
            Some(Host::Ipv6(address)) => vec![IpAddr::V6(address)],
            Some(Host::Domain(domain)) if domain.eq_ignore_ascii_case("localhost") => {
                return Self::Loopback;
            }
            Some(Host::Domain(domain)) => {
                let port = url.port_or_known_default().unwrap_or(443);
                match tokio::net::lookup_host((domain, port)).await {
                    Ok(addresses) => addresses.map(|address| address.ip()).collect(),
                    Err(_) => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        addresses
            .into_iter()
            .filter_map(|address| match classify(address) {
                AddressClass::Loopback => Some(Self::Loopback),
                AddressClass::PrivateNetwork => Some(Self::PrivateNetwork),
                AddressClass::Public | AddressClass::Blocked => None,
            })
            .max()
            .unwrap_or(Self::Public)
    }

    fn allows(self, address: IpAddr) -> bool {
        match classify(address) {
            AddressClass::Public => true,
            AddressClass::PrivateNetwork => self >= Self::PrivateNetwork,
            AddressClass::Loopback => self >= Self::Loopback,
            AddressClass::Blocked => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressClass {
    Public,
    PrivateNetwork,
    Loopback,
    Blocked,
}

/// Cloud metadata services outside the link-local range.
const METADATA_ADDRESSES: [IpAddr; 3] = [
    // Alibaba Cloud, inside carrier-grade NAT space.
    IpAddr::V4(Ipv4Addr::new(100, 100, 100, 200)),
    // Azure WireServer.
    IpAddr::V4(Ipv4Addr::new(168, 63, 129, 16)),
    // AWS over IPv6, inside unique-local space.
    IpAddr::V6(Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254)),
];

fn classify(address: IpAddr) -> AddressClass {
    match address {
        IpAddr::V4(address) => classify_v4(address),
        IpAddr::V6(address) => classify_v6(address),
    }
}

fn classify_v4(address: Ipv4Addr) -> AddressClass {
    let [a, b, c, _] = address.octets();
    if METADATA_ADDRESSES.contains(&IpAddr::V4(address)) {
        return AddressClass::Blocked;
    }
    match (a, b, c) {
        (127, _, _) => AddressClass::Loopback,
        (10, _, _) | (192, 168, _) => AddressClass::PrivateNetwork,
        (172, 16..=31, _) | (100, 64..=127, _) => AddressClass::PrivateNetwork,
        // This network, link-local (cloud metadata), IETF protocol
        // assignments, documentation, benchmarking, 6to4 relays, and the
        // reserved and multicast ranges up to the broadcast address.
        (0, _, _)
        | (169, 254, _)
        | (192, 0, 0 | 2)
        | (192, 88, 99)
        | (198, 18..=19, _)
        | (198, 51, 100)
        | (203, 0, 113)
        | (224..=255, _, _) => AddressClass::Blocked,
        _ => AddressClass::Public,
    }
}

fn classify_v6(address: Ipv6Addr) -> AddressClass {
    // Addresses that embed an IPv4 address are judged by it: mapped,
    // NAT64 (64:ff9b::/96), and 6to4 (2002::/16).
    if let Some(v4) = address.to_ipv4_mapped() {
        return classify_v4(v4);
    }
    let segments = address.segments();
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return classify_v4(Ipv4Addr::from(
            (u32::from(segments[6]) << 16) | u32::from(segments[7]),
        ));
    }
    if segments[0] == 0x2002 {
        return classify_v4(Ipv4Addr::from(
            (u32::from(segments[1]) << 16) | u32::from(segments[2]),
        ));
    }
    if METADATA_ADDRESSES.contains(&IpAddr::V6(address)) {
        return AddressClass::Blocked;
    }
    if address.is_loopback() {
        return AddressClass::Loopback;
    }
    match segments[0] {
        // Unique local, and the deprecated site-local range.
        0xfc00..=0xfdff | 0xfec0..=0xfeff => AddressClass::PrivateNetwork,
        // Link-local and multicast.
        0xfe80..=0xfebf | 0xff00..=0xffff => AddressClass::Blocked,
        // Teredo, which tunnels to an IPv4 address the guard cannot see, and
        // documentation.
        0x2001 if segments[1] == 0 || segments[1] == 0x0db8 => AddressClass::Blocked,
        // Global unicast.
        0x2000..=0x3fff => AddressClass::Public,
        // Unspecified, the old IPv4-compatible range, discard-only, and
        // everything not yet allocated.
        _ => AddressClass::Blocked,
    }
}

/// Why an outbound fetch did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundFetchError {
    /// The URL is not one the fetcher may request: wrong scheme, user
    /// information, or no host.
    UnsupportedUrl(String),
    /// The host is, or resolves to, an address outside the allowed scope.
    BlockedAddress { host: String },
    /// The host did not resolve.
    Unresolved { host: String },
    /// The body was larger than the fetcher's limit.
    TooLarge,
    /// The connection failed or timed out.
    Transport(String),
}

impl OutboundFetchError {
    /// Whether retrying later could succeed.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::Unresolved { .. })
    }
}

impl fmt::Display for OutboundFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedUrl(reason) => formatter.write_str(reason),
            Self::BlockedAddress { host } => {
                write!(
                    formatter,
                    "`{host}` resolves to an address that is not allowed"
                )
            }
            Self::Unresolved { host } => write!(formatter, "`{host}` could not be resolved"),
            Self::TooLarge => formatter.write_str("the response is too large"),
            Self::Transport(reason) => write!(formatter, "the request failed: {reason}"),
        }
    }
}

impl StdError for OutboundFetchError {}

/// Raised inside the resolver and found again in the request error's chain.
#[derive(Debug)]
struct ResolverRejection(OutboundFetchError);

impl fmt::Display for ResolverRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl StdError for ResolverRejection {}

#[derive(Debug)]
struct GuardedResolver {
    scope: OutboundAddressScope,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let scope = self.scope;
        let host = name.as_str().to_string();
        Box::pin(async move {
            let rejection = |error: OutboundFetchError| -> Box<dyn StdError + Send + Sync> {
                Box::new(ResolverRejection(error))
            };
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|_| rejection(OutboundFetchError::Unresolved { host: host.clone() }))?
                .collect();
            if addresses.is_empty() {
                return Err(rejection(OutboundFetchError::Unresolved { host }));
            }
            // One disallowed answer rejects the name: a host that mixes public
            // and internal addresses is not one to trust with either.
            if addresses.iter().any(|address| !scope.allows(address.ip())) {
                return Err(rejection(OutboundFetchError::BlockedAddress { host }));
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

/// An HTTP client that only connects to addresses within its scope.
#[derive(Clone)]
pub struct OutboundFetcher {
    client: reqwest::Client,
    scope: OutboundAddressScope,
    timeout: Duration,
    max_body_bytes: usize,
}

impl fmt::Debug for OutboundFetcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundFetcher")
            .field("scope", &self.scope)
            .field("timeout", &self.timeout)
            .field("max_body_bytes", &self.max_body_bytes)
            .finish_non_exhaustive()
    }
}

impl OutboundFetcher {
    #[must_use]
    pub fn new(scope: OutboundAddressScope) -> Self {
        Self::with_limits(
            scope,
            DEFAULT_OUTBOUND_TIMEOUT,
            DEFAULT_OUTBOUND_MAX_BODY_BYTES,
        )
    }

    #[must_use]
    pub fn with_limits(
        scope: OutboundAddressScope,
        timeout: Duration,
        max_body_bytes: usize,
    ) -> Self {
        let client = reqwest::Client::builder()
            .dns_resolver(Arc::new(GuardedResolver { scope }))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .referer(false)
            .connect_timeout(timeout)
            .timeout(timeout)
            .user_agent(concat!("oceans-llm/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("outbound fetch HTTP client configuration must be valid");
        Self {
            client,
            scope,
            timeout,
            max_body_bytes,
        }
    }

    #[must_use]
    pub fn scope(&self) -> OutboundAddressScope {
        self.scope
    }

    /// Checks what can be checked before connecting: the scheme, user
    /// information, and an IP-literal host. Names are checked when they
    /// resolve.
    pub fn check_url(&self, url: &Url) -> Result<(), OutboundFetchError> {
        if !url.username().is_empty() || url.password().is_some() {
            return Err(OutboundFetchError::UnsupportedUrl(
                "the URL must not include user information".to_string(),
            ));
        }
        let host = url
            .host()
            .ok_or_else(|| OutboundFetchError::UnsupportedUrl("the URL has no host".to_string()))?;
        let literal = match host {
            Host::Ipv4(address) => Some(IpAddr::V4(address)),
            Host::Ipv6(address) => Some(IpAddr::V6(address)),
            Host::Domain(_) => None,
        };
        let loopback = match literal {
            Some(address) => classify(address) == AddressClass::Loopback,
            None => {
                matches!(host, Host::Domain(domain) if domain.eq_ignore_ascii_case("localhost"))
            }
        };
        match url.scheme() {
            "https" => {}
            "http" if loopback && self.scope == OutboundAddressScope::Loopback => {}
            _ => {
                return Err(OutboundFetchError::UnsupportedUrl(
                    "the URL must use https".to_string(),
                ));
            }
        }
        if let Some(address) = literal
            && !self.scope.allows(address)
        {
            return Err(OutboundFetchError::BlockedAddress {
                host: address.to_string(),
            });
        }
        Ok(())
    }

    /// Starts a request to `url` once [`Self::check_url`] accepts it.
    pub fn request(
        &self,
        method: Method,
        url: &Url,
    ) -> Result<reqwest::RequestBuilder, OutboundFetchError> {
        self.check_url(url)?;
        Ok(self.client.request(method, url.clone()))
    }

    /// Sends a request built by [`Self::request`].
    pub async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<OutboundResponse, OutboundFetchError> {
        let response = request.send().await.map_err(request_error)?;
        Ok(OutboundResponse {
            response,
            max_body_bytes: self.max_body_bytes,
        })
    }

    /// `GET`s `url` with an `Accept` header.
    pub async fn get(
        &self,
        url: &Url,
        accept: &str,
    ) -> Result<OutboundResponse, OutboundFetchError> {
        let request = self
            .request(Method::GET, url)?
            .header(reqwest::header::ACCEPT, accept);
        self.send(request).await
    }
}

/// One fetcher per address scope, sharing limits, for requests made on
/// behalf of an admin-registered server.
#[derive(Debug, Clone)]
pub struct ScopedOutboundFetchers {
    fetchers: Arc<[OutboundFetcher; 3]>,
}

impl ScopedOutboundFetchers {
    #[must_use]
    pub fn new(timeout: Duration, max_body_bytes: usize) -> Self {
        let fetcher = |scope| OutboundFetcher::with_limits(scope, timeout, max_body_bytes);
        Self {
            fetchers: Arc::new([
                fetcher(OutboundAddressScope::Public),
                fetcher(OutboundAddressScope::PrivateNetwork),
                fetcher(OutboundAddressScope::Loopback),
            ]),
        }
    }

    #[must_use]
    pub fn for_scope(&self, scope: OutboundAddressScope) -> &OutboundFetcher {
        let index = match scope {
            OutboundAddressScope::Public => 0,
            OutboundAddressScope::PrivateNetwork => 1,
            OutboundAddressScope::Loopback => 2,
        };
        &self.fetchers[index]
    }

    /// The fetcher scoped to the admin-entered `server_url`'s address class.
    /// An unparseable URL gets the public scope.
    pub async fn for_trusted_url(&self, server_url: &str) -> &OutboundFetcher {
        let scope = match Url::parse(server_url) {
            Ok(url) => OutboundAddressScope::of_trusted_url(&url).await,
            Err(_) => OutboundAddressScope::Public,
        };
        self.for_scope(scope)
    }
}

/// A response whose body has not been read yet. The body is read only on
/// request, and never past the fetcher's size limit.
#[derive(Debug)]
pub struct OutboundResponse {
    response: reqwest::Response,
    max_body_bytes: usize,
}

impl OutboundResponse {
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }

    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        self.response.headers()
    }

    pub async fn body(mut self) -> Result<Vec<u8>, OutboundFetchError> {
        if self
            .response
            .content_length()
            .is_some_and(|length| length > self.max_body_bytes as u64)
        {
            return Err(OutboundFetchError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = self.response.chunk().await.map_err(request_error)? {
            if body.len() + chunk.len() > self.max_body_bytes {
                return Err(OutboundFetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

fn request_error(error: reqwest::Error) -> OutboundFetchError {
    let mut source = error.source();
    while let Some(current) = source {
        if let Some(ResolverRejection(rejection)) = current.downcast_ref::<ResolverRejection>() {
            return rejection.clone();
        }
        source = current.source();
    }
    let reason = if error.is_timeout() {
        "timed out".to_string()
    } else {
        error.without_url().to_string()
    };
    OutboundFetchError::Transport(reason)
}

#[cfg(test)]
mod tests;
