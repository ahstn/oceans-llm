//! Per-source refresh: each catalog source has its own cache row, ETag or checksum,
//! `fetched_at` generation and process-local check time, so one failing source never
//! blocks the others.

use std::{
    collections::BTreeMap,
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};

use gateway_core::{GatewayError, PricingCatalogCacheRecord, PricingCatalogRepository, StoreError};
use reqwest::header::IF_NONE_MATCH;
use time::OffsetDateTime;
use tracing::{debug, warn};

use super::{
    PricingCatalog, PricingCatalogDocument, PricingCatalogSnapshotFile,
    PricingCatalogSnapshotMetadata, ProjectedSnapshot, document_checksum, projected_snapshot,
    sources::{CatalogSourceId, SourceDescriptor},
};
use crate::catalog_fetch::{CatalogFetch, CatalogFetchError, fetch_bounded};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RefreshMode {
    /// Refresh only sources whose snapshot is older than the refresh interval, with
    /// `If-None-Match` where the source sent an ETag.
    IfStale,
    /// Refresh every source unconditionally.
    Forced,
}

/// What a refresh did to one source's snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingCatalogRefreshOutcome {
    /// A changed document was cached as the source's next generation.
    Updated,
    /// The source answered 304, or its document's checksum matched the cached one.
    Unchanged,
    /// The refresh failed; the source keeps its cached or vendored snapshot.
    Failed(PricingCatalogRefreshError),
}

impl PricingCatalogRefreshOutcome {
    /// The stable outcome code: `updated`, `unchanged` or `failed`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::Failed(_) => "failed",
        }
    }
}

/// Why a source refresh failed, without upstream response text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingCatalogRefreshError {
    RequestFailed,
    UnexpectedStatus,
    ResponseTooLarge,
    ResponseReadFailed,
    InvalidResponse,
    StoreFailed,
}

impl PricingCatalogRefreshError {
    /// The stable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RequestFailed => "request_failed",
            Self::UnexpectedStatus => "unexpected_status",
            Self::ResponseTooLarge => "response_too_large",
            Self::ResponseReadFailed => "response_read_failed",
            Self::InvalidResponse => "invalid_response",
            Self::StoreFailed => "store_failed",
        }
    }
}

/// One source's result of a forced refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingCatalogSourceRefresh {
    pub source: CatalogSourceId,
    pub outcome: PricingCatalogRefreshOutcome,
    /// Generation of the source snapshot now in use, cached or vendored.
    pub fetched_at: OffsetDateTime,
}

/// One source's fetch target, vendored fallback and process-local check time.
pub(super) struct CatalogSource {
    pub(super) descriptor: &'static SourceDescriptor,
    pub(super) url: String,
    pub(super) fallback: PricingCatalogSnapshotFile,
    /// When this process last completed a request to the source, including a 304 or an
    /// unchanged 200 that left the cache row untouched.
    ///
    /// Such a request leaves `fetched_at` as is: it is the generation pricing-row
    /// provenance is reconciled against, and the store's only cache write is a CAS that
    /// must advance it, so recording the check there would re-stamp pricing rows. This
    /// instead keeps the source from being requested again before the interval passes.
    last_checked: Mutex<Option<Instant>>,
}

impl CatalogSource {
    pub(super) fn new(
        descriptor: &'static SourceDescriptor,
        url: String,
        fallback: PricingCatalogSnapshotFile,
    ) -> Self {
        Self {
            descriptor,
            url,
            fallback,
            last_checked: Mutex::new(None),
        }
    }

    /// The source with its production URL and checked-in snapshot.
    pub(super) fn vendored(source: CatalogSourceId) -> Self {
        let descriptor = source.descriptor();
        Self::new(
            descriptor,
            descriptor.url.to_string(),
            descriptor.vendored_snapshot(),
        )
    }

    fn id(&self) -> CatalogSourceId {
        self.descriptor.id
    }

    fn checked_within(&self, interval: Duration) -> bool {
        self.last_checked
            .lock()
            .expect("lock poisoned")
            .is_some_and(|checked| checked.elapsed() < interval)
    }

    fn mark_checked(&self) {
        *self.last_checked.lock().expect("lock poisoned") = Some(Instant::now());
    }
}

/// A source's cache row as read from the store.
pub(super) struct StoredSource {
    /// The row's generation, which the next cache write must advance; `None` without a
    /// row.
    fetched_at: Option<OffsetDateTime>,
    /// The row's snapshot; `None` without a row or when its document does not parse.
    snapshot: Option<PricingCatalogSnapshotFile>,
}

impl StoredSource {
    /// The snapshot the source contributes: the cached one, else the vendored one.
    pub(super) fn into_snapshot(self, source: &CatalogSource) -> PricingCatalogSnapshotFile {
        self.snapshot.unwrap_or_else(|| source.fallback.clone())
    }

    fn in_use_metadata<'src>(
        &'src self,
        source: &'src CatalogSource,
    ) -> &'src PricingCatalogSnapshotMetadata {
        self.snapshot
            .as_ref()
            .map_or(&source.fallback.metadata, |snapshot| &snapshot.metadata)
    }

    fn is_invalid(&self) -> bool {
        self.fetched_at.is_some() && self.snapshot.is_none()
    }
}

#[derive(Debug, thiserror::Error)]
enum SourceRefreshFailure {
    #[error(transparent)]
    Fetch(#[from] CatalogFetchError),
    #[error("failed projecting the response: {0}")]
    Project(GatewayError),
    #[error("failed caching the snapshot: {0}")]
    Store(GatewayError),
}

impl SourceRefreshFailure {
    fn error(&self) -> PricingCatalogRefreshError {
        match self {
            Self::Fetch(CatalogFetchError::Request(_)) => PricingCatalogRefreshError::RequestFailed,
            Self::Fetch(CatalogFetchError::Status(_)) => {
                PricingCatalogRefreshError::UnexpectedStatus
            }
            Self::Fetch(CatalogFetchError::TooLarge { .. }) => {
                PricingCatalogRefreshError::ResponseTooLarge
            }
            Self::Fetch(CatalogFetchError::Body(_)) => {
                PricingCatalogRefreshError::ResponseReadFailed
            }
            Self::Project(_) => PricingCatalogRefreshError::InvalidResponse,
            Self::Store(_) => PricingCatalogRefreshError::StoreFailed,
        }
    }
}

impl<R> PricingCatalog<R>
where
    R: PricingCatalogRepository + Send + Sync + 'static,
{
    /// Refresh the sources `mode` selects, concurrently. Returns the outcome of each
    /// source that was attempted, in [`CatalogSourceId::ALL`] order; failures are
    /// logged with their cause.
    pub(super) async fn refresh_sources(
        &self,
        mode: RefreshMode,
    ) -> Vec<(CatalogSourceId, PricingCatalogRefreshOutcome)> {
        let stored = join3(
            self.sources
                .each_ref()
                .map(|source| self.load_stored_source(source)),
        )
        .await;
        // A new source generation is newer than every snapshot in use, so the merged
        // generation advances whenever any source changes.
        let generation = self
            .sources
            .iter()
            .zip(&stored)
            .filter_map(|(source, stored)| {
                let stored = stored.as_ref().ok()?;
                Some(stored.in_use_metadata(source).fetched_at)
            })
            .max();
        let mut stored = stored.into_iter();
        let outcomes = join3(self.sources.each_ref().map(|source| {
            let stored = stored.next().expect("one stored row per source");
            self.refresh_source(source, stored, generation, mode)
        }))
        .await;
        self.sources
            .iter()
            .zip(outcomes)
            .filter_map(|(source, outcome)| Some((source.id(), outcome?)))
            .collect()
    }

    /// Load every source's snapshot: its cached one, else its vendored one.
    pub(super) async fn load_source_snapshots(
        &self,
    ) -> Result<BTreeMap<CatalogSourceId, PricingCatalogSnapshotFile>, GatewayError> {
        let stored = join3(
            self.sources
                .each_ref()
                .map(|source| self.load_stored_source(source)),
        )
        .await;
        self.sources
            .iter()
            .zip(stored)
            .map(|(source, stored)| Ok((source.id(), stored?.into_snapshot(source))))
            .collect()
    }

    /// `None` when `mode` skips a source that is not stale.
    async fn refresh_source(
        &self,
        source: &CatalogSource,
        stored: Result<StoredSource, GatewayError>,
        generation: Option<OffsetDateTime>,
        mode: RefreshMode,
    ) -> Option<PricingCatalogRefreshOutcome> {
        let result = match stored {
            Ok(stored) => {
                if mode == RefreshMode::IfStale && !self.is_stale(source, &stored) {
                    return None;
                }
                self.fetch_and_cache(source, stored, generation, mode).await
            }
            Err(error) => Err(SourceRefreshFailure::Store(error)),
        };
        Some(match result {
            Ok(outcome) => {
                source.mark_checked();
                outcome
            }
            Err(failure) => {
                warn!(
                    source = %source.id(),
                    source_url = %source.url,
                    error = %failure,
                    "catalog source refresh failed; keeping its current snapshot"
                );
                PricingCatalogRefreshOutcome::Failed(failure.error())
            }
        })
    }

    fn is_stale(&self, source: &CatalogSource, stored: &StoredSource) -> bool {
        // A row that no longer parses is repaired at once.
        if stored.is_invalid() {
            return true;
        }
        if source.checked_within(self.refresh_interval) {
            return false;
        }
        let age_secs = OffsetDateTime::now_utc()
            .unix_timestamp()
            .saturating_sub(stored.in_use_metadata(source).fetched_at.unix_timestamp());
        age_secs >= i64::try_from(self.refresh_interval.as_secs()).unwrap_or(i64::MAX)
    }

    /// Fetch the source and cache a changed projection as its next generation.
    ///
    /// A 304, or a 200 whose projection has the cached snapshot's checksum, leaves the
    /// cache row untouched, ETag and `fetched_at` included. OpenRouter sends no ETag, so
    /// the checksum is its only no-op check.
    async fn fetch_and_cache(
        &self,
        source: &CatalogSource,
        stored: StoredSource,
        generation: Option<OffsetDateTime>,
        mode: RefreshMode,
    ) -> Result<PricingCatalogRefreshOutcome, SourceRefreshFailure> {
        let mut request = self.client.get(&source.url);
        if mode == RefreshMode::IfStale
            && let Some(etag) = stored
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.metadata.etag.as_deref())
        {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let CatalogFetch::Fetched(body) =
            fetch_bounded(request, source.descriptor.max_bytes).await?
        else {
            return Ok(PricingCatalogRefreshOutcome::Unchanged);
        };

        let providers =
            (source.descriptor.project)(&body.bytes).map_err(SourceRefreshFailure::Project)?;
        let fetched_at = next_catalog_generation_at(
            generation.max(stored.fetched_at),
            OffsetDateTime::now_utc(),
        );
        let ProjectedSnapshot {
            snapshot,
            document_json,
        } = projected_snapshot(
            providers,
            source.descriptor.remote_source,
            body.etag,
            fetched_at,
        )
        .map_err(SourceRefreshFailure::Project)?;
        if stored
            .snapshot
            .as_ref()
            .is_some_and(|cached| cached.metadata.checksum == snapshot.metadata.checksum)
        {
            debug!(
                source = %source.id(),
                checksum = ?snapshot.metadata.checksum,
                "catalog source projection unchanged; keeping the cached snapshot generation"
            );
            return Ok(PricingCatalogRefreshOutcome::Unchanged);
        }

        let written = self
            .repo
            .compare_and_swap_pricing_catalog_cache(
                &PricingCatalogCacheRecord {
                    catalog_key: source.descriptor.cache_key.to_string(),
                    source: snapshot.metadata.source,
                    etag: snapshot.metadata.etag,
                    fetched_at,
                    snapshot_json: document_json,
                },
                stored.fetched_at,
            )
            .await
            .map_err(|error| SourceRefreshFailure::Store(error.into()))?;
        if !written {
            self.confirm_superseding_cache_write(source, stored.fetched_at)
                .await
                .map_err(SourceRefreshFailure::Store)?;
        }
        Ok(PricingCatalogRefreshOutcome::Updated)
    }

    /// A lost CAS is fine when another writer advanced the row past `expected_fetched_at`.
    async fn confirm_superseding_cache_write(
        &self,
        source: &CatalogSource,
        expected_fetched_at: Option<OffsetDateTime>,
    ) -> Result<(), GatewayError> {
        let latest = self.load_stored_source(source).await?.fetched_at;
        let was_superseded = match expected_fetched_at {
            Some(expected) => latest.is_some_and(|fetched_at| fetched_at > expected),
            None => latest.is_some(),
        };
        if was_superseded {
            Ok(())
        } else {
            Err(StoreError::PricingSyncConflict.into())
        }
    }

    async fn load_stored_source(
        &self,
        source: &CatalogSource,
    ) -> Result<StoredSource, GatewayError> {
        let catalog_key = source.descriptor.cache_key;
        let Some(cache) = self.repo.get_pricing_catalog_cache(catalog_key).await? else {
            return Ok(StoredSource {
                fetched_at: None,
                snapshot: None,
            });
        };
        let fetched_at = cache.fetched_at;
        let snapshot = match serde_json::from_str::<PricingCatalogDocument>(&cache.snapshot_json) {
            Ok(document) => Some(PricingCatalogSnapshotFile {
                metadata: PricingCatalogSnapshotMetadata {
                    checksum: Some(document_checksum(&cache.snapshot_json)),
                    source: cache.source,
                    etag: cache.etag,
                    fetched_at,
                },
                providers: document.providers,
            }),
            Err(error) => {
                warn!(
                    catalog_key,
                    error = %error,
                    "cached catalog source snapshot is invalid; using its vendored snapshot"
                );
                None
            }
        };
        Ok(StoredSource {
            fetched_at: Some(fetched_at),
            snapshot,
        })
    }
}

/// The next generation after `current_fetched_at`: `now`, or one second later than the
/// current generation when the clock has not passed it.
pub(super) fn next_catalog_generation_at(
    current_fetched_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> OffsetDateTime {
    let current_timestamp = current_fetched_at
        .map(OffsetDateTime::unix_timestamp)
        .unwrap_or(i64::MIN);
    let next_timestamp = now
        .unix_timestamp()
        .max(current_timestamp.saturating_add(1));

    OffsetDateTime::from_unix_timestamp(next_timestamp)
        .expect("catalog generation timestamp remains in range")
}

/// Await one future per source concurrently.
async fn join3<F: Future>([first, second, third]: [F; 3]) -> [F::Output; 3] {
    let (first, second, third) = tokio::join!(first, second, third);
    [first, second, third]
}
