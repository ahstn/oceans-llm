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
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

use async_trait::async_trait;
use gateway_core::{
    GatewayOauthClientAuthMethod, GatewayOauthClientRecord, GatewayOauthRepository, StoreError,
};
use reqwest::header::{AGE, CACHE_CONTROL, DATE};
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
const MAX_CACHED_DOCUMENTS: usize = 1024;
/// Documents fetched at once. A request beyond this fails as temporarily
/// unavailable instead of queueing, so callers cannot pile up fetches.
const MAX_CONCURRENT_FETCHES: usize = 32;
const FETCH_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;

/// A fetched document, before validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedDocument {
    pub status: u16,
    pub cache_control: Option<String>,
    /// The response's `Age` in seconds: how long a cache in between held it.
    pub age_seconds: Option<i64>,
    /// The response's `Date`, when the origin generated it.
    pub date: Option<OffsetDateTime>,
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
        // Several field lines form one comma-separated list (RFC 9110 §5.2).
        let cache_control = response
            .headers()
            .get_all(CACHE_CONTROL)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();
        let cache_control = (!cache_control.is_empty()).then(|| cache_control.join(", "));
        let age_seconds = response
            .headers()
            .get(AGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| delta_seconds(value.trim()));
        let date = response
            .headers()
            .get(DATE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| httpdate::parse_http_date(value).ok())
            .map(OffsetDateTime::from);
        let body = if status == 200 {
            response.body().await?
        } else {
            Vec::new()
        };
        Ok(FetchedDocument {
            status,
            cache_control,
            age_seconds,
            date,
            body,
        })
    }
}

#[derive(Debug, Clone)]
struct CachedClient {
    client: GatewayOauthClientRecord,
    fresh_until: OffsetDateTime,
    /// False when the document said `must-revalidate` or `proxy-revalidate`:
    /// once stale, it may only be used again after a successful fetch.
    reusable_when_stale: bool,
}

impl CachedClient {
    /// Until when the copy may stand in for an unreachable host.
    fn usable_until(&self) -> OffsetDateTime {
        self.fresh_until + MAX_STALE_IF_ERROR
    }
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
            fetch_permits: Semaphore::new(MAX_CONCURRENT_FETCHES),
        })
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
    /// With `stored_fallback`, the stored row stands in when the document
    /// cannot be fetched or no longer validates. Revocation uses it, since
    /// ending a grant is safe whatever the document now says.
    pub(crate) async fn resolve<R>(
        &self,
        repo: &R,
        client_id: &str,
        now: OffsetDateTime,
        stored_fallback: bool,
    ) -> Result<GatewayOauthClientRecord, OauthError>
    where
        R: GatewayOauthRepository + ?Sized,
    {
        if let Some(client) = self.cached(client_id, now) {
            return Ok(client);
        }
        let flight = InFlight::join(&self.in_flight, client_id);
        let outcome = flight
            .cell
            .get_or_init(|| self.refresh(repo, client_id, now))
            .await
            .clone();
        drop(flight);
        let failure = match outcome {
            Ok(client) => return Ok(client),
            Err(failure) => failure,
        };
        if failure.transient
            && let Some(mut stale) = self.stale(client_id, now)
        {
            tracing::warn!(
                client_id,
                error = %failure.error.description,
                "using the cached copy of an unavailable client metadata document"
            );
            // Write the copy back first: cleanup ages these rows by their last
            // write, and requests and grants need the row to exist.
            stale.updated_at = now;
            return repo
                .upsert_gateway_oauth_metadata_document_client(&stale)
                .await
                .map_err(OauthError::from);
        }
        if stored_fallback && let Some(stored) = stored_client(repo, client_id).await? {
            return Ok(stored);
        }
        Err(failure.error)
    }

    /// Fetches, validates, and stores the document, and caches it for as
    /// long as its headers allow.
    async fn refresh<R>(&self, repo: &R, client_id: &str, now: OffsetDateTime) -> RefreshOutcome
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
            })?;
        let fetched = self.fetch_and_validate(client_id, now).await?;
        let stored = repo
            .upsert_gateway_oauth_metadata_document_client(&fetched.client)
            .await
            .map_err(|error| {
                ResolveFailure::permanent(match error {
                    StoreError::Conflict(_) => {
                        OauthError::invalid_client("client_id is already registered another way")
                    }
                    error => error.into(),
                })
            })?;
        if fetched.ttl > Duration::ZERO {
            self.remember(
                stored.clone(),
                now,
                now + fetched.ttl,
                fetched.reusable_when_stale,
            );
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
    fn stale(&self, client_id: &str, now: OffsetDateTime) -> Option<GatewayOauthClientRecord> {
        let cache = self.cache.lock().expect("metadata document cache lock");
        cache
            .get(client_id)
            .filter(|entry| entry.reusable_when_stale && entry.usable_until() > now)
            .map(|entry| entry.client.clone())
    }

    fn remember(
        &self,
        client: GatewayOauthClientRecord,
        now: OffsetDateTime,
        fresh_until: OffsetDateTime,
        reusable_when_stale: bool,
    ) {
        let mut cache = self.cache.lock().expect("metadata document cache lock");
        if cache.len() >= MAX_CACHED_DOCUMENTS && !cache.contains_key(&client.client_id) {
            cache.retain(|_, entry| entry.usable_until() > now);
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
                reusable_when_stale,
            },
        );
    }

    fn forget(&self, client_id: &str) {
        self.cache
            .lock()
            .expect("metadata document cache lock")
            .remove(client_id);
    }

    async fn fetch_and_validate(
        &self,
        client_id: &str,
        now: OffsetDateTime,
    ) -> Result<ValidatedDocument, ResolveFailure> {
        let url = parse_client_id_url(client_id).map_err(ResolveFailure::permanent)?;
        if !self.host_is_allowed(&url) {
            return Err(ResolveFailure::permanent(OauthError::invalid_client(
                "client metadata documents from this host are not accepted",
            )));
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
            });
        }
        let client =
            validate_document(client_id, &fetched.body, now).map_err(ResolveFailure::permanent)?;
        // RFC 9111 §4.2.3: the response is at least as old as its `Age`, and
        // as the time since its `Date`.
        let apparent_age = fetched
            .date
            .map_or(0, |date| (now - date).whole_seconds().max(0));
        let age = fetched.age_seconds.unwrap_or(0).max(apparent_age);
        Ok(ValidatedDocument {
            client,
            ttl: cache_ttl(fetched.cache_control.as_deref(), Some(age)),
            reusable_when_stale: !must_revalidate(fetched.cache_control.as_deref()),
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
    reusable_when_stale: bool,
}

#[derive(Clone)]
struct ResolveFailure {
    error: OauthError,
    transient: bool,
}

impl ResolveFailure {
    fn permanent(error: OauthError) -> Self {
        Self {
            error,
            transient: false,
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
    client_id.starts_with("https://")
}

/// The host that published a metadata document client, for the consent
/// screen.
#[must_use]
pub fn metadata_document_host(client: &GatewayOauthClientRecord) -> Option<String> {
    if client.registration_kind != METADATA_DOCUMENT_REGISTRATION_KIND {
        return None;
    }
    Url::parse(&client.client_id)
        .ok()?
        .host_str()
        .map(ToOwned::to_owned)
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
    let Some(rest) = client_id.strip_prefix("https://") else {
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
    let path = rest
        .split(['?', '#'])
        .next()
        .and_then(|authority_and_path| authority_and_path.split_once('/'))
        .map_or("", |(_, path)| path);
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
    if url.path() == "/" {
        return Err(invalid("it must include a path"));
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

/// How long a document stays fresh. `no-store`, `no-cache`, and `max-age=0`
/// mean fetching it each time; otherwise `max-age` applies, less the time a
/// cache in between already held it (`Age`), capped at [`MAX_CACHE_TTL`].
/// A malformed or repeated `max-age` makes the response stale (RFC 9111
/// §4.2.1). `Expires` is not read.
fn cache_ttl(cache_control: Option<&str>, age_seconds: Option<i64>) -> Duration {
    let Some(cache_control) = cache_control else {
        return DEFAULT_CACHE_TTL;
    };
    let mut max_age = None;
    for directive in cache_control.split(',') {
        let directive = directive.trim().to_ascii_lowercase();
        if directive == "no-store" || directive == "no-cache" {
            return Duration::ZERO;
        }
        if let Some(raw) = directive.strip_prefix("max-age=") {
            // One optional pair of quotes around ASCII digits, nothing else.
            let unquoted = raw
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(raw);
            let Some(seconds) = delta_seconds(unquoted) else {
                return Duration::ZERO;
            };
            if max_age.replace(seconds).is_some() {
                return Duration::ZERO;
            }
        }
    }
    max_age.map_or(DEFAULT_CACHE_TTL, |seconds| {
        let remaining = seconds.saturating_sub(age_seconds.unwrap_or(0).max(0));
        Duration::seconds(remaining.max(0)).min(MAX_CACHE_TTL)
    })
}

/// Whether the document forbids reuse once stale without a successful fetch
/// (`must-revalidate`, or `proxy-revalidate` from a cache in between).
fn must_revalidate(cache_control: Option<&str>) -> bool {
    cache_control.is_some_and(|value| {
        value.split(',').any(|directive| {
            let directive = directive.trim();
            directive.eq_ignore_ascii_case("must-revalidate")
                || directive.eq_ignore_ascii_case("proxy-revalidate")
        })
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
