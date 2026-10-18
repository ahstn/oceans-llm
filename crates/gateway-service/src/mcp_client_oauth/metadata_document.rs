//! Client ID Metadata Documents: a client whose `client_id` is an https URL
//! publishes its metadata at that URL, and the gateway fetches it instead of
//! asking the client to register.
//!
//! The gateway only learns that whoever controls the URL's host published
//! the document, so the consent screen names that host and says the client is
//! unverified. Fetches go through an [`OutboundFetcher`] that reaches public
//! addresses only. A validated document is stored as a `metadata_document`
//! client row, which requests and grants reference, and cached in memory for
//! as long as its `Cache-Control` allows, within [`MAX_CACHE_TTL`].

use std::{
    collections::HashMap,
    fmt,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration as StdDuration, Instant},
};

use async_trait::async_trait;
use gateway_core::{
    GatewayOauthClientAuthMethod, GatewayOauthClientRecord, GatewayOauthRepository, StoreError,
};
use reqwest::header::{AGE, CACHE_CONTROL, DATE, EXPIRES, HeaderMap, HeaderName, VARY};
use serde::Deserialize;
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use tokio::sync::{OnceCell, Semaphore};
use url::{Host, Url};

use super::{
    error::OauthError,
    redirect::validate_redirect_uri,
    registration::{
        MAX_REDIRECT_URIS, MAX_URI_LENGTH, optional_https_uri, optional_text, validate_grant_types,
        validate_response_types,
    },
    source_limits::{RateLimited, SourceRateLimiter, SourceRateLimits},
};
use crate::outbound_fetch::{
    InvalidDomainName, OutboundAddressScope, OutboundFetchError, OutboundFetcher,
    host_matches_domains, normalize_domains,
};

pub const METADATA_DOCUMENT_REGISTRATION_KIND: &str = "metadata_document";
/// Used when the response has no `Cache-Control` freshness.
pub const DEFAULT_CACHE_TTL: Duration = Duration::minutes(10);
/// The longest the gateway trusts a document without fetching it again.
pub const MAX_CACHE_TTL: Duration = Duration::hours(24);
/// How long past its freshness a cached document may stand in while its
/// host is temporarily unreachable (stale-if-error).
pub const MAX_STALE_IF_ERROR: Duration = Duration::hours(24);
/// After a cached copy stands in for an unreachable host, how long it is
/// served before the host is tried again.
const STAND_IN_RETRY_AFTER: Duration = Duration::seconds(60);
const MAX_CACHED_DOCUMENTS: usize = 1024;
/// Documents fetched at once. A request beyond this fails as temporarily
/// unavailable instead of queueing, so callers cannot pile up fetches.
const MAX_CONCURRENT_FETCHES: usize = 32;
/// How long a document that was refused for good (missing, invalid, or not
/// allowed) is refused again without another fetch.
const FAILURE_TTL: Duration = Duration::seconds(30);
const FETCH_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
/// Fetches allowed per minute, per requesting address and across the gateway.
/// Each new URL misses every cache, so without this one caller could make
/// the gateway fetch and store a client for as many URLs as it can invent.
pub const DEFAULT_FETCH_RATE_LIMITS: SourceRateLimits = SourceRateLimits {
    per_source_per_minute: 30,
    per_minute: 300,
};

/// A fetched document, before validation. The headers decide how long it
/// may be cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedDocument {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// Where documents come from: the network in production, a fixture in tests.
#[async_trait]
pub trait ClientMetadataDocumentSource: Send + Sync {
    async fn fetch(&self, url: &Url) -> Result<FetchedDocument, OutboundFetchError>;
}

#[async_trait]
impl ClientMetadataDocumentSource for OutboundFetcher {
    async fn fetch(&self, url: &Url) -> Result<FetchedDocument, OutboundFetchError> {
        let response = self.get(url, "application/json").await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = if status == 200 {
            response.body().await?
        } else {
            Vec::new()
        };
        Ok(FetchedDocument {
            status,
            headers,
            body,
        })
    }
}

#[derive(Debug, Clone)]
struct CachedClient {
    client: GatewayOauthClientRecord,
    fresh_until: OffsetDateTime,
    /// Until when the copy may stand in for an unreachable host: its
    /// freshness plus its [`stale_window`]. Set from the last successful
    /// fetch, so standing in never extends it.
    usable_until: OffsetDateTime,
}

/// What a client is being loaded for, which decides what may stand in for
/// a metadata document that cannot be used right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientLookup {
    /// Authorization, code exchange, and refresh: only a cacheable copy
    /// stands in, and only while the document's host is unreachable.
    Use,
    /// Revocation, which is always safe: the stored row also stands in when
    /// the document is gone, no longer validates, or documents are off.
    Revoke,
}

/// Resolves metadata document clients and caches what it fetched.
pub struct ClientMetadataDocuments {
    allowed_domains: Vec<String>,
    source: Arc<dyn ClientMetadataDocumentSource>,
    cache: Mutex<HashMap<String, CachedClient>>,
    /// One refresh per client at a time: concurrent requests for a stale
    /// document wait for the same fetch and share its result.
    in_flight: Flights,
    fetch_permits: Semaphore,
    /// Documents recently refused for good, so repeating the request does
    /// not fetch again until [`FAILURE_TTL`] passes.
    failures: Mutex<HashMap<String, (ResolveFailure, OffsetDateTime)>>,
    fetch_limiter: SourceRateLimiter,
}

type RefreshOutcome = Result<GatewayOauthClientRecord, ResolveFailure>;

impl fmt::Debug for ClientMetadataDocuments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientMetadataDocuments")
            .field("allowed_domains", &self.allowed_domains)
            .finish_non_exhaustive()
    }
}

impl ClientMetadataDocuments {
    /// Fetches over the network. An empty `allowed_domains` allows any host.
    pub fn new(allowed_domains: Vec<String>) -> Result<Self, InvalidDomainName> {
        let fetcher = OutboundFetcher::with_limits(
            OutboundAddressScope::Public,
            FETCH_TIMEOUT,
            MAX_DOCUMENT_BYTES,
        );
        Self::with_source(allowed_domains, Arc::new(fetcher))
    }

    pub fn with_source(
        allowed_domains: Vec<String>,
        source: Arc<dyn ClientMetadataDocumentSource>,
    ) -> Result<Self, InvalidDomainName> {
        Ok(Self {
            allowed_domains: normalize_domains(allowed_domains)?,
            source,
            cache: Mutex::new(HashMap::new()),
            in_flight: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            fetch_permits: Semaphore::new(MAX_CONCURRENT_FETCHES),
            fetch_limiter: SourceRateLimiter::new(DEFAULT_FETCH_RATE_LIMITS),
        })
    }

    /// Limits document fetches per requesting address and across the
    /// gateway. Requests answered from a cache, joining a fetch in flight, or
    /// refused before fetching (an invalid or disallowed client ID) do not
    /// count. A refused fetch falls back to a cached copy as an unreachable
    /// host does.
    pub fn with_fetch_rate_limits(
        mut self,
        limits: SourceRateLimits,
    ) -> Result<Self, super::McpClientOauthSettingsError> {
        super::settings::validate_rate_limits(limits)?;
        self.fetch_limiter = SourceRateLimiter::new(limits);
        Ok(self)
    }

    #[must_use]
    pub fn allowed_domains(&self) -> &[String] {
        &self.allowed_domains
    }

    /// Returns the client for `client_id`, fetching its document when the
    /// cached copy is stale.
    ///
    /// A transient fetch failure falls back to this process's last cached
    /// copy, for up to [`MAX_STALE_IF_ERROR`] past its freshness. A document
    /// served `no-store` or `no-cache` is never cached, so it never stands in.
    ///
    /// For [`ClientLookup::Revoke`], the stored row stands in when the
    /// document cannot be fetched or no longer validates, since ending a
    /// grant is safe whatever the document now says.
    pub(crate) async fn resolve<R>(
        &self,
        repo: &R,
        client_id: &str,
        now: OffsetDateTime,
        lookup: ClientLookup,
        requester: Option<IpAddr>,
    ) -> Result<GatewayOauthClientRecord, OauthError>
    where
        R: GatewayOauthRepository + ?Sized,
    {
        if let Some(client) = self.cached(client_id, now) {
            return Ok(client);
        }
        let outcome = match self.recent_failure(client_id, now) {
            Some(failure) => Err(failure),
            None => {
                let flight = InFlight::join(&self.in_flight, client_id);
                let outcome = flight
                    .outcome(self.refresh(repo, client_id, now, requester))
                    .await;
                drop(flight);
                outcome
            }
        };
        let failure = match outcome {
            Ok(client) => return Ok(client),
            Err(failure) => failure,
        };
        if lookup == ClientLookup::Revoke
            && let Some(stored) = stored_client(repo, client_id).await?
        {
            return Ok(stored);
        }
        Err(failure.error)
    }

    /// Fetches, validates, stores, and caches the document. When the host is
    /// temporarily unreachable, or the fetch limits refuse the fetch, a cached
    /// copy stands in instead. This runs once per refresh shared by concurrent
    /// callers, so a burst during an outage costs one fetch and one write.
    async fn refresh<R>(
        &self,
        repo: &R,
        client_id: &str,
        now: OffsetDateTime,
        requester: Option<IpAddr>,
    ) -> RefreshOutcome
    where
        R: GatewayOauthRepository + ?Sized,
    {
        match self.fetch_and_store(repo, client_id, now, requester).await {
            Err(failure) if failure.transient => self
                .stand_in(repo, client_id, now, &failure)
                .await
                .unwrap_or(Err(failure)),
            Err(failure) if failure.document_refused => {
                // The document is gone or no longer valid: a later outage must
                // not bring the old copy back.
                self.forget(client_id);
                self.remember_failure(client_id, &failure, now);
                Err(failure)
            }
            // A store error says nothing about the document: keep the cached
            // copy and retry on the next request.
            outcome => outcome,
        }
    }

    /// Serves the cached copy for a document that cannot be fetched just now,
    /// if one may stand in.
    /// It is written back first, since cleanup ages these rows by their last
    /// write and requests and grants need the row, then served for
    /// [`STAND_IN_RETRY_AFTER`] before the host is tried again.
    async fn stand_in<R>(
        &self,
        repo: &R,
        client_id: &str,
        now: OffsetDateTime,
        failure: &ResolveFailure,
    ) -> Option<RefreshOutcome>
    where
        R: GatewayOauthRepository + ?Sized,
    {
        let (mut client, usable_until) = self.stale(client_id, now)?;
        tracing::warn!(
            client_id,
            error = %failure.error.description,
            "using the cached copy of an unavailable client metadata document"
        );
        client.updated_at = now;
        let stored = match repo
            .upsert_gateway_oauth_metadata_document_client(&client)
            .await
        {
            Ok(stored) => stored,
            Err(error) => return Some(Err(ResolveFailure::store(error))),
        };
        self.serve_stand_in(
            stored.clone(),
            (now + STAND_IN_RETRY_AFTER).min(usable_until),
        );
        Some(Ok(stored))
    }

    async fn fetch_and_store<R>(
        &self,
        repo: &R,
        client_id: &str,
        now: OffsetDateTime,
        requester: Option<IpAddr>,
    ) -> RefreshOutcome
    where
        R: GatewayOauthRepository + ?Sized,
    {
        let _permit = self
            .fetch_permits
            .try_acquire()
            .map_err(|_| ResolveFailure {
                error: OauthError::temporarily_unavailable(
                    "too many client metadata documents are being fetched; try again shortly",
                ),
                transient: true,
                document_refused: false,
            })?;
        let fetched = self.fetch_and_validate(client_id, now, requester).await?;
        let stored = repo
            .upsert_gateway_oauth_metadata_document_client(&fetched.client)
            .await
            .map_err(|error| match error {
                // A registered client owns this ID, which no document changes.
                StoreError::Conflict(_) => ResolveFailure::permanent(OauthError::invalid_client(
                    "client_id is already registered another way",
                )),
                error => ResolveFailure::store(error),
            })?;
        if fetched.ttl > Duration::ZERO {
            self.remember(stored.clone(), now, now + fetched.ttl, fetched.stale_window);
        } else {
            self.forget(client_id);
        }
        Ok(stored)
    }

    fn cached(&self, client_id: &str, now: OffsetDateTime) -> Option<GatewayOauthClientRecord> {
        let cache = self.cache.lock().expect("metadata document cache lock");
        cache
            .get(client_id)
            .filter(|entry| entry.fresh_until > now)
            .map(|entry| entry.client.clone())
    }

    /// A cached copy past its freshness that may still stand in for an
    /// unreachable host.
    fn stale(
        &self,
        client_id: &str,
        now: OffsetDateTime,
    ) -> Option<(GatewayOauthClientRecord, OffsetDateTime)> {
        let cache = self.cache.lock().expect("metadata document cache lock");
        cache
            .get(client_id)
            .filter(|entry| entry.usable_until > now)
            .map(|entry| (entry.client.clone(), entry.usable_until))
    }

    /// Serves a stand-in as fresh until `fresh_until`, keeping the entry's
    /// stale-if-error limit from its last successful fetch.
    fn serve_stand_in(&self, client: GatewayOauthClientRecord, fresh_until: OffsetDateTime) {
        let mut cache = self.cache.lock().expect("metadata document cache lock");
        if let Some(entry) = cache.get_mut(&client.client_id) {
            entry.client = client;
            entry.fresh_until = fresh_until;
        }
    }

    fn remember(
        &self,
        client: GatewayOauthClientRecord,
        now: OffsetDateTime,
        fresh_until: OffsetDateTime,
        stale_window: Duration,
    ) {
        let mut cache = self.cache.lock().expect("metadata document cache lock");
        if cache.len() >= MAX_CACHED_DOCUMENTS && !cache.contains_key(&client.client_id) {
            cache.retain(|_, entry| entry.usable_until > now);
            if cache.len() >= MAX_CACHED_DOCUMENTS
                && let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.fresh_until)
                    .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            client.client_id.clone(),
            CachedClient {
                client,
                fresh_until,
                usable_until: fresh_until + stale_window,
            },
        );
    }

    fn recent_failure(&self, client_id: &str, now: OffsetDateTime) -> Option<ResolveFailure> {
        let failures = self
            .failures
            .lock()
            .expect("metadata document failure lock");
        failures
            .get(client_id)
            .filter(|(_, until)| *until > now)
            .map(|(failure, _)| failure.clone())
    }

    fn remember_failure(&self, client_id: &str, failure: &ResolveFailure, now: OffsetDateTime) {
        let mut failures = self
            .failures
            .lock()
            .expect("metadata document failure lock");
        if failures.len() >= MAX_CACHED_DOCUMENTS && !failures.contains_key(client_id) {
            failures.retain(|_, (_, until)| *until > now);
            if failures.len() >= MAX_CACHED_DOCUMENTS {
                return;
            }
        }
        failures.insert(client_id.to_string(), (failure.clone(), now + FAILURE_TTL));
    }

    fn forget(&self, client_id: &str) {
        self.cache
            .lock()
            .expect("metadata document cache lock")
            .remove(client_id);
    }

    /// Fetches and checks the document. Client IDs refused without a fetch
    /// are refused before the fetch limits count, so they cannot spend the
    /// gateway-wide budget that documents on allowed hosts need.
    async fn fetch_and_validate(
        &self,
        client_id: &str,
        now: OffsetDateTime,
        requester: Option<IpAddr>,
    ) -> Result<ValidatedDocument, ResolveFailure> {
        let url = parse_client_id_url(client_id).map_err(ResolveFailure::permanent)?;
        if !self.host_is_allowed(&url) {
            return Err(ResolveFailure::permanent(OauthError::invalid_client(
                "client metadata documents from this host are not accepted",
            )));
        }
        if let Err(limited) = self.fetch_limiter.check(requester, Instant::now()) {
            tracing::warn!(?requester, "client metadata document fetch rate limited");
            return Err(ResolveFailure::rate_limited(limited));
        }
        let fetched = self.source.fetch(&url).await.map_err(|error| {
            let transient = error.is_transient();
            let description = format!("client metadata document could not be fetched: {error}");
            ResolveFailure {
                error: if transient {
                    OauthError::temporarily_unavailable(description)
                } else {
                    OauthError::invalid_client(description)
                },
                transient,
                document_refused: !transient,
            }
        })?;
        if fetched.status != 200 {
            let description = format!("client metadata document returned HTTP {}", fetched.status);
            // A timeout or rate limit passes like an outage does.
            let transient = matches!(fetched.status, 408 | 429) || fetched.status >= 500;
            return Err(ResolveFailure {
                error: if transient {
                    OauthError::temporarily_unavailable(description)
                } else {
                    OauthError::invalid_client(description)
                },
                transient,
                document_refused: !transient,
            });
        }
        let client =
            validate_document(client_id, &fetched.body, now).map_err(ResolveFailure::permanent)?;
        let (ttl, stale_window) = freshness(&fetched.headers, now);
        Ok(ValidatedDocument {
            client,
            ttl,
            stale_window,
        })
    }

    fn host_is_allowed(&self, url: &Url) -> bool {
        if self.allowed_domains.is_empty() {
            return true;
        }
        let Some(Host::Domain(host)) = url.host() else {
            return false;
        };
        host_matches_domains(host, &self.allowed_domains)
    }
}

type Flights = Mutex<HashMap<String, Flight>>;

/// A refresh in flight and how many requests are waiting on it.
#[derive(Default)]
struct Flight {
    cell: Arc<OnceCell<RefreshOutcome>>,
    callers: usize,
}

/// A caller's share of a refresh in flight. The entry stays while any caller
/// holds a share, so a cancelled waiter cannot let a second, overlapping
/// refresh start. Dropping the last share, whether the refresh finished or
/// the request was cancelled, removes the entry.
struct InFlight<'a> {
    map: &'a Flights,
    client_id: &'a str,
    cell: Arc<OnceCell<RefreshOutcome>>,
}

impl<'a> InFlight<'a> {
    fn join(map: &'a Flights, client_id: &'a str) -> Self {
        let mut flights = map.lock().expect("metadata document flight lock");
        let flight = flights.entry(client_id.to_string()).or_default();
        flight.callers += 1;
        Self {
            map,
            client_id,
            cell: flight.cell.clone(),
        }
    }

    /// The shared refresh's outcome, running `refresh` if no caller has
    /// settled it yet. A fetch-limit refusal belongs to the caller it
    /// refused, whose source may be the only one over the limits, so it is
    /// not shared: the next waiting caller runs a refresh of its own.
    async fn outcome(&self, refresh: impl Future<Output = RefreshOutcome>) -> RefreshOutcome {
        let shared = self
            .cell
            .get_or_try_init(|| async {
                match refresh.await {
                    Err(failure) if failure.is_rate_limited() => Err(failure),
                    outcome => Ok(outcome),
                }
            })
            .await;
        match shared {
            Ok(outcome) => outcome.clone(),
            Err(failure) => Err(failure),
        }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let Ok(mut flights) = self.map.lock() else {
            return;
        };
        let Some(flight) = flights.get_mut(self.client_id) else {
            return;
        };
        if !Arc::ptr_eq(&flight.cell, &self.cell) {
            return;
        }
        flight.callers -= 1;
        if flight.callers == 0 {
            flights.remove(self.client_id);
        }
    }
}

struct ValidatedDocument {
    client: GatewayOauthClientRecord,
    ttl: Duration,
    stale_window: Duration,
}

#[derive(Clone)]
struct ResolveFailure {
    error: OauthError,
    /// The document could not be fetched just now (its host was unreachable,
    /// or the fetch limits refused), so a cached copy may stand in.
    transient: bool,
    /// The document itself was refused (missing, invalid, or not allowed),
    /// so the cached copy is dropped and the refusal remembered briefly.
    document_refused: bool,
}

impl ResolveFailure {
    fn permanent(error: OauthError) -> Self {
        Self {
            error,
            transient: false,
            document_refused: true,
        }
    }

    /// The fetch limits refused the fetch: retryable, and a cached copy may
    /// stand in, as for an unreachable host.
    fn rate_limited(limited: RateLimited) -> Self {
        Self {
            error: OauthError::rate_limited(
                "too many client metadata documents requested; try again later",
                limited.retry_after_seconds(),
            ),
            transient: true,
            document_refused: false,
        }
    }

    fn is_rate_limited(&self) -> bool {
        self.error.retry_after_seconds.is_some()
    }

    /// The store failed: neither the host's fault nor the document's.
    fn store(error: StoreError) -> Self {
        Self {
            error: error.into(),
            transient: false,
            document_refused: false,
        }
    }
}

async fn stored_client<R>(
    repo: &R,
    client_id: &str,
) -> Result<Option<GatewayOauthClientRecord>, OauthError>
where
    R: GatewayOauthRepository + ?Sized,
{
    Ok(repo
        .get_gateway_oauth_client(client_id)
        .await?
        .filter(|client| client.registration_kind == METADATA_DOCUMENT_REGISTRATION_KIND))
}

/// Whether `client_id` names a metadata document rather than a registration.
/// Registered clients get UUIDs, so an https URL is never one of them.
#[must_use]
pub fn is_metadata_document_client_id(client_id: &str) -> bool {
    https_rest(client_id).is_some()
}

/// What follows `https://`, matching the scheme case-insensitively
/// (RFC 3986 §3.1). The `client_id` itself is kept as sent.
fn https_rest(client_id: &str) -> Option<&str> {
    let (scheme, rest) = client_id.split_at_checked("https://".len())?;
    scheme.eq_ignore_ascii_case("https://").then_some(rest)
}

/// The host that published a metadata document client, for the consent
/// screen.
#[must_use]
pub fn metadata_document_host(client: &GatewayOauthClientRecord) -> Option<String> {
    if client.registration_kind != METADATA_DOCUMENT_REGISTRATION_KIND {
        return None;
    }
    let url = Url::parse(&client.client_id).ok()?;
    let host = url.host_str()?;
    // A non-default port can belong to someone other than the site's owner,
    // so the consent screen names it.
    Some(
        url.port()
            .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}")),
    )
}

/// The client ID rules of the metadata document draft: https, a path, and no
/// fragment, user information, or dot segments. The `client_id` string itself
/// is what the document must repeat and what is stored, so forms the URL
/// parser would rewrite (such as an explicit `:443`) stay distinct clients.
fn parse_client_id_url(client_id: &str) -> Result<Url, OauthError> {
    let invalid = |reason: &str| {
        OauthError::invalid_client(format!(
            "client_id is not a valid metadata document URL: {reason}"
        ))
    };
    if client_id.len() > MAX_URI_LENGTH {
        return Err(invalid("it is too long"));
    }
    let Some(rest) = https_rest(client_id) else {
        return Err(invalid("it must use https"));
    };
    if client_id
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '\\')
    {
        return Err(invalid(
            "it must not contain whitespace, control characters, or backslashes",
        ));
    }
    // The parser removes dot segments, including percent-encoded ones, so
    // look for them in the string as sent.
    let authority_and_path = rest.split(['?', '#']).next().unwrap_or_default();
    // A path is required. `/` alone is valid, though the draft advises
    // against it.
    let Some((_, path)) = authority_and_path.split_once('/') else {
        return Err(invalid("it must include a path"));
    };
    if path.split('/').any(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            "." | ".." | "%2e" | "%2e%2e" | ".%2e" | "%2e."
        )
    }) {
        return Err(invalid("it must not contain dot segments"));
    }
    let url = Url::parse(client_id).map_err(|_| invalid("it is not an absolute URL"))?;
    if url.scheme() != "https" || url.host().is_none() {
        return Err(invalid("it must use https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("it must not include user information"));
    }
    if url.fragment().is_some() {
        return Err(invalid("it must not include a fragment"));
    }
    Ok(url)
}

#[derive(Debug, Default, Deserialize)]
struct ClientMetadataDocument {
    client_id: Option<String>,
    client_name: Option<String>,
    client_uri: Option<String>,
    logo_uri: Option<String>,
    redirect_uris: Option<Vec<String>>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
    token_endpoint_auth_method: Option<String>,
    software_id: Option<String>,
    software_version: Option<String>,
    client_secret: Option<Value>,
    client_secret_expires_at: Option<Value>,
}

/// Checks a fetched document and turns it into the client row to store.
fn validate_document(
    client_id: &str,
    body: &[u8],
    now: OffsetDateTime,
) -> Result<GatewayOauthClientRecord, OauthError> {
    let invalid = |reason: String| {
        OauthError::invalid_client(format!("client metadata document is invalid: {reason}"))
    };
    let document: ClientMetadataDocument =
        serde_json::from_slice(body).map_err(|_| invalid("it is not a JSON object".into()))?;
    if document.client_id.as_deref() != Some(client_id) {
        return Err(invalid("its client_id does not match its URL".into()));
    }
    if document.client_secret.is_some() || document.client_secret_expires_at.is_some() {
        return Err(invalid("it must not contain a client secret".into()));
    }
    match document.token_endpoint_auth_method.as_deref() {
        None | Some("none") => {}
        Some(_) => {
            return Err(invalid(
                "token_endpoint_auth_method must be none, since the gateway shares no secret with it"
                    .into(),
            ));
        }
    }
    let to_invalid = |error: OauthError| invalid(error.description);
    let client_name = optional_text(document.client_name, "client_name")
        .map_err(to_invalid)?
        .ok_or_else(|| invalid("client_name is required".into()))?;
    let redirect_uris = document.redirect_uris.unwrap_or_default();
    if redirect_uris.is_empty() {
        return Err(invalid("redirect_uris is required".into()));
    }
    if redirect_uris.len() > MAX_REDIRECT_URIS {
        return Err(invalid(format!(
            "at most {MAX_REDIRECT_URIS} redirect URIs are allowed"
        )));
    }
    for redirect_uri in &redirect_uris {
        validate_redirect_uri(redirect_uri).map_err(to_invalid)?;
    }
    let grant_types = validate_grant_types(document.grant_types).map_err(to_invalid)?;
    validate_response_types(document.response_types).map_err(to_invalid)?;
    Ok(GatewayOauthClientRecord {
        client_id: client_id.to_string(),
        registration_kind: METADATA_DOCUMENT_REGISTRATION_KIND.to_string(),
        client_name: Some(client_name),
        client_uri: optional_https_uri(document.client_uri, "client_uri").map_err(to_invalid)?,
        logo_uri: optional_https_uri(document.logo_uri, "logo_uri").map_err(to_invalid)?,
        redirect_uris,
        grant_types,
        token_endpoint_auth_method: GatewayOauthClientAuthMethod::None,
        client_secret_hash: None,
        software_id: optional_text(document.software_id, "software_id").map_err(to_invalid)?,
        software_version: optional_text(document.software_version, "software_version")
            .map_err(to_invalid)?,
        created_at: now,
        updated_at: now,
    })
}

/// How long a fetched document may be cached, and how long past that it may
/// stand in for an unreachable host, from its response headers.
fn freshness(headers: &HeaderMap, now: OffsetDateTime) -> (Duration, Duration) {
    let field = |name: HeaderName| {
        // Several field lines form one comma-separated list (RFC 9110 §5.2).
        let lines = headers
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();
        (!lines.is_empty()).then(|| lines.join(", "))
    };
    let http_date = |value: &str| {
        httpdate::parse_http_date(value.trim())
            .ok()
            .map(OffsetDateTime::from)
    };
    // `Vary: *` never matches a later request (RFC 9111 §4.1).
    if field(VARY).is_some_and(|vary| vary.split(',').any(|item| item.trim() == "*")) {
        return (Duration::ZERO, Duration::ZERO);
    }
    let cache_control = field(CACHE_CONTROL);
    let date = field(DATE).and_then(|value| http_date(&value));
    // RFC 9111 §4.2.3: the response is at least as old as its `Age`, and as
    // the time since its `Date`.
    let apparent_age = date.map_or(0, |date| (now - date).whole_seconds().max(0));
    // `Age` is a singleton: a repeated one uses its first member (RFC 9111
    // §5.1), so it is not joined like the list-valued fields.
    let age = headers
        .get(AGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(|value| delta_seconds(value.trim()))
        .unwrap_or(0)
        .max(apparent_age);
    // RFC 9111 §4.2.1: `Expires` less `Date`; an invalid one is already past.
    let expires_lifetime = field(EXPIRES).map(|value| {
        http_date(&value).map_or(0, |expires| (expires - date.unwrap_or(now)).whole_seconds())
    });
    (
        cache_ttl(cache_control.as_deref(), Some(age), expires_lifetime),
        stale_window(cache_control.as_deref()),
    )
}

/// The `Cache-Control` directives, as lowercase names with their raw values.
fn directives(cache_control: &str) -> impl Iterator<Item = (String, Option<&str>)> {
    cache_control
        .split(',')
        .map(str::trim)
        .filter(|directive| !directive.is_empty())
        .map(|directive| match directive.split_once('=') {
            Some((name, value)) => (name.trim().to_ascii_lowercase(), Some(value.trim())),
            None => (directive.to_ascii_lowercase(), None),
        })
}

/// A directive's delta-seconds value: one optional pair of quotes around
/// ASCII digits, nothing else. A missing or malformed value is `None`.
fn directive_seconds(value: Option<&str>) -> Option<i64> {
    let raw = value?;
    let unquoted = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(raw);
    delta_seconds(unquoted)
}

/// How long a document stays fresh, read as a shared cache would (RFC 9111),
/// since one copy serves every user of this process.
///
/// `no-store`, `no-cache`, and `private` mean fetching it each time.
/// Otherwise `s-maxage` applies, else `max-age`, else the `Expires`
/// lifetime, less the time a cache in between already held it (`Age`),
/// capped at [`MAX_CACHE_TTL`]. A malformed, valueless, or repeated
/// `max-age` or `s-maxage` makes the response stale (RFC 9111 §4.2.1).
/// Without any of them, it is [`DEFAULT_CACHE_TTL`].
fn cache_ttl(
    cache_control: Option<&str>,
    age_seconds: Option<i64>,
    expires_lifetime: Option<i64>,
) -> Duration {
    let mut max_age = None;
    let mut s_maxage = None;
    for (name, value) in directives(cache_control.unwrap_or_default()) {
        let slot = match name.as_str() {
            "no-store" | "no-cache" | "private" => return Duration::ZERO,
            "s-maxage" => &mut s_maxage,
            "max-age" => &mut max_age,
            _ => continue,
        };
        let Some(seconds) = directive_seconds(value) else {
            return Duration::ZERO;
        };
        if slot.replace(seconds).is_some() {
            return Duration::ZERO;
        }
    }
    s_maxage
        .or(max_age)
        .or(expires_lifetime)
        .map_or(DEFAULT_CACHE_TTL, |seconds| {
            let remaining = seconds.saturating_sub(age_seconds.unwrap_or(0).max(0));
            Duration::seconds(remaining.max(0)).min(MAX_CACHE_TTL)
        })
}

/// How long past its freshness a document may stand in for an unreachable
/// host. None after `must-revalidate`, `proxy-revalidate`, or `s-maxage`
/// (which implies `proxy-revalidate` for a shared cache). Otherwise the
/// document's `stale-if-error` (RFC 5861), or [`MAX_STALE_IF_ERROR`] without
/// one, capped at [`MAX_STALE_IF_ERROR`]; a malformed or repeated
/// `stale-if-error` allows none.
fn stale_window(cache_control: Option<&str>) -> Duration {
    let mut stale_if_error = None;
    for (name, value) in directives(cache_control.unwrap_or_default()) {
        match name.as_str() {
            "must-revalidate" | "proxy-revalidate" | "s-maxage" => return Duration::ZERO,
            "stale-if-error" => {
                let Some(seconds) = directive_seconds(value) else {
                    return Duration::ZERO;
                };
                if stale_if_error.replace(seconds).is_some() {
                    return Duration::ZERO;
                }
            }
            _ => {}
        }
    }
    stale_if_error.map_or(MAX_STALE_IF_ERROR, |seconds| {
        Duration::seconds(seconds).min(MAX_STALE_IF_ERROR)
    })
}

/// An HTTP delta-seconds value: ASCII digits only, so `+5`, `-5`, and `5s`
/// are malformed. Values too large for `i64` saturate.
fn delta_seconds(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(raw.parse::<i64>().unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests;
