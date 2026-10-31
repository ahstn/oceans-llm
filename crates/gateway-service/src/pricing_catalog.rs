use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context;
use gateway_core::{
    GatewayError, ModelPricingProvenanceUpdate, ModelPricingRecord, ModelPricingSyncChanges,
    ModelRoute, Money4, PricingCatalogCacheRecord, PricingCatalogRepository, PricingLimits,
    PricingModalities, PricingProvenance, PricingResolution, PricingUnpricedReason,
    ProviderConnection, ResolvedModelPricing, StoreError,
};
use reqwest::{Client, header::IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use serde_json::Number;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    admin_audit::AuditProgress,
    catalog_fetch::{CatalogFetch, FetchedBody, catalog_http_client, fetch_bounded},
};

pub const DEFAULT_PRICING_CATALOG_SOURCE_URL: &str = "https://models.dev/api.json";
pub const PRICING_CATALOG_CACHE_KEY: &str = "models_dev_supported_v3";
const PREVIOUS_PRICING_CATALOG_CACHE_KEY: &str = "models_dev_supported_v2";
pub const DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// The raw models.dev `api.json` is around 6 MB; anything far larger is not the expected
/// payload.
const MAX_MODELS_DEV_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const REMOTE_SOURCE: &str = "models_dev_api";
const VENDORED_SOURCE: &str = "vendored_models_dev";
const VENDORED_FALLBACK_JSON: &str = include_str!("../data/pricing_catalog_fallback.json");
const MAX_PRICING_SYNC_ATTEMPTS: usize = 3;

pub mod metadata;
mod target;

#[cfg(test)]
use target::normalize_bedrock_pricing_model_id;
use target::{PricingTarget, pricing_target_for_route};
pub use target::{SUPPORTED_PRICING_PROVIDER_IDS, is_supported_pricing_provider_id};
pub(crate) use target::{
    catalog_metadata_target_for_route, catalog_model_id_fallback,
    catalog_pricing_supported_for_route,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshMode {
    Conditional,
    Unconditional,
}

#[derive(Clone)]
pub struct PricingCatalog<R> {
    repo: Arc<R>,
    client: Client,
    source_url: String,
    catalog_key: String,
    refresh_interval: Duration,
    fallback_snapshot: PricingCatalogSnapshot,
    metadata_snapshot: Arc<tokio::sync::Mutex<Option<Arc<PricingCatalogSnapshot>>>>,
    /// When this process last completed a refresh request, including a 304 or an unchanged
    /// 200 that left the cache row untouched. See [`Self::refresh_remote_snapshot`].
    last_checked: Arc<Mutex<Option<Instant>>>,
}

impl<R> PricingCatalog<R>
where
    R: PricingCatalogRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self::with_options(
            repo,
            DEFAULT_PRICING_CATALOG_SOURCE_URL.to_string(),
            PRICING_CATALOG_CACHE_KEY.to_string(),
            DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL,
        )
    }

    #[must_use]
    pub fn with_options(
        repo: Arc<R>,
        source_url: String,
        catalog_key: String,
        refresh_interval: Duration,
    ) -> Self {
        Self {
            repo,
            client: catalog_http_client(),
            source_url,
            catalog_key,
            refresh_interval,
            fallback_snapshot: load_vendored_fallback_snapshot(),
            metadata_snapshot: Default::default(),
            last_checked: Default::default(),
        }
    }

    #[cfg(test)]
    fn with_fallback_snapshot(
        repo: Arc<R>,
        source_url: String,
        catalog_key: String,
        refresh_interval: Duration,
        fallback_snapshot: PricingCatalogSnapshot,
    ) -> Self {
        Self {
            repo,
            client: catalog_http_client(),
            source_url,
            catalog_key,
            refresh_interval,
            fallback_snapshot,
            metadata_snapshot: Default::default(),
            last_checked: Default::default(),
        }
    }

    pub async fn refresh_if_stale(&self) -> Result<(), GatewayError> {
        let current = self.load_stored_cache().await?;
        let cache_is_invalid = current
            .as_ref()
            .is_some_and(|cache| !cached_document_is_valid(cache));
        if !cache_is_invalid && self.checked_within_refresh_interval() {
            return Ok(());
        }
        let current_fetched_at = current
            .as_ref()
            .filter(|cache| cached_document_is_valid(cache))
            .map(|cache| cache.fetched_at)
            .unwrap_or(self.fallback_snapshot.metadata.fetched_at);
        let now = OffsetDateTime::now_utc();
        if !cache_is_invalid
            && now
                .unix_timestamp()
                .saturating_sub(current_fetched_at.unix_timestamp())
                < self.refresh_interval.as_secs() as i64
        {
            return Ok(());
        }

        self.refresh_remote_snapshot(current, RefreshMode::Conditional)
            .await
    }

    pub async fn refresh_now_and_sync(&self) -> Result<(), GatewayError> {
        self.refresh_now_and_sync_tracked(&AuditProgress::default())
            .await
    }

    /// Like [`Self::refresh_now_and_sync`]; `progress` is marked once the fetched
    /// catalog is cached, so an audit entry can record a refresh whose sync fails.
    pub async fn refresh_now_and_sync_tracked(
        &self,
        progress: &AuditProgress,
    ) -> Result<(), GatewayError> {
        let current = self.load_stored_cache().await?;
        self.refresh_remote_snapshot(current, RefreshMode::Unconditional)
            .await?;
        progress.landed();
        self.sync_latest_snapshot_with_retry().await
    }

    /// Fetch the source and cache a changed projection as the next snapshot generation.
    ///
    /// A 304, or a 200 whose projection matches the stored snapshot, leaves the cache row
    /// untouched, ETag and `fetched_at` included. `fetched_at` is the snapshot generation
    /// that pricing-row provenance is reconciled against, and the store's only cache write
    /// is a CAS that must advance it, so recording the check there would re-stamp every
    /// pricing row. The process-local `last_checked` instead keeps such a source from being
    /// requested again before the refresh interval passes.
    async fn refresh_remote_snapshot(
        &self,
        current: Option<PricingCatalogCacheRecord>,
        mode: RefreshMode,
    ) -> Result<(), GatewayError> {
        let mut request = self.client.get(&self.source_url);
        if mode == RefreshMode::Conditional
            && let Some(etag) = current
                .as_ref()
                .filter(|cache| cached_document_is_valid(cache))
                .and_then(|cache| cache.etag.clone())
        {
            request = request.header(IF_NONE_MATCH, etag);
        }

        let fetched = fetch_bounded(request, MAX_MODELS_DEV_RESPONSE_BYTES)
            .await
            .map_err(|error| {
                GatewayError::Internal(format!("pricing catalog refresh failed: {error}"))
            })?;
        if let CatalogFetch::Fetched(body) = fetched {
            self.store_fetched_snapshot(current, body).await?;
        }
        *self.metadata_snapshot.lock().await = None;
        *self.last_checked.lock().expect("lock poisoned") = Some(Instant::now());
        Ok(())
    }

    async fn store_fetched_snapshot(
        &self,
        current: Option<PricingCatalogCacheRecord>,
        body: FetchedBody,
    ) -> Result<(), GatewayError> {
        let expected_fetched_at = current.as_ref().map(|cache| cache.fetched_at);
        let fetched_at = next_catalog_generation_at(expected_fetched_at, OffsetDateTime::now_utc());
        let ProjectedSnapshot {
            snapshot,
            document_json,
        } = project_models_dev_snapshot(&body.bytes, REMOTE_SOURCE, body.etag, fetched_at)?;
        // The stored JSON is the canonical document its checksum covers, so equal bytes
        // mean an equal checksum without hashing the stored row.
        if current
            .as_ref()
            .is_some_and(|cache| cache.snapshot_json == document_json)
        {
            debug!(
                catalog_key = %self.catalog_key,
                checksum = ?snapshot.metadata.checksum,
                "pricing catalog projection unchanged; keeping the stored snapshot generation"
            );
            return Ok(());
        }

        let stored = self
            .repo
            .compare_and_swap_pricing_catalog_cache(
                &PricingCatalogCacheRecord {
                    catalog_key: self.catalog_key.clone(),
                    source: snapshot.metadata.source,
                    etag: snapshot.metadata.etag,
                    fetched_at,
                    snapshot_json: document_json,
                },
                expected_fetched_at,
            )
            .await?;
        if !stored {
            self.confirm_superseding_cache_write(expected_fetched_at)
                .await?;
        }
        Ok(())
    }

    fn checked_within_refresh_interval(&self) -> bool {
        self.last_checked
            .lock()
            .expect("lock poisoned")
            .is_some_and(|checked| checked.elapsed() < self.refresh_interval)
    }

    async fn confirm_superseding_cache_write(
        &self,
        expected_fetched_at: Option<OffsetDateTime>,
    ) -> Result<(), GatewayError> {
        let latest = self.load_stored_cache().await?;
        let was_superseded = match expected_fetched_at {
            Some(expected) => latest.is_some_and(|cache| cache.fetched_at > expected),
            None => latest.is_some(),
        };

        if was_superseded {
            Ok(())
        } else {
            Err(StoreError::PricingSyncConflict.into())
        }
    }

    pub async fn resolve_for_provider_connection(
        &self,
        provider: &ProviderConnection,
        route: &ModelRoute,
        occurred_at: OffsetDateTime,
    ) -> Result<PricingResolution, GatewayError> {
        let (pricing_provider_id, model_id) = match pricing_target_for_route(provider, route) {
            PricingTarget::Exact {
                pricing_provider_id,
                model_id,
            } => (pricing_provider_id, model_id),
            PricingTarget::Unpriced(reason) => {
                return Ok(PricingResolution::Unpriced { reason });
            }
        };

        let Some(record) = resolve_catalog_pricing_record(
            self.repo.as_ref(),
            &pricing_provider_id,
            &model_id,
            occurred_at,
        )
        .await?
        else {
            return Ok(PricingResolution::Unpriced {
                reason: PricingUnpricedReason::ModelNotFound,
            });
        };

        Ok(PricingResolution::Exact {
            pricing: Box::new(resolved_model_pricing(&record)),
        })
    }

    pub async fn refresh_if_stale_and_sync(&self) -> Result<(), GatewayError> {
        if let Err(error) = self.refresh_if_stale().await {
            warn!(
                catalog_key = %self.catalog_key,
                source_url = %self.source_url,
                error = %error,
                "pricing catalog refresh failed; falling back to cached snapshot"
            );
        }

        self.sync_latest_snapshot_with_retry().await
    }

    async fn sync_latest_snapshot_with_retry(&self) -> Result<(), GatewayError> {
        for attempt in 1..=MAX_PRICING_SYNC_ATTEMPTS {
            // Re-read on each reconciliation attempt, including changes from other replicas.
            let snapshot = {
                let mut cached = self.metadata_snapshot.lock().await;
                let snapshot = Arc::new(self.load_snapshot_from_store_or_fallback().await?);
                *cached = Some(Arc::clone(&snapshot));
                snapshot
            };
            match self.sync_model_pricing_snapshot(&snapshot).await {
                Err(GatewayError::Store(StoreError::PricingSyncConflict))
                    if attempt < MAX_PRICING_SYNC_ATTEMPTS =>
                {
                    warn!(
                        catalog_key = %self.catalog_key,
                        attempt,
                        "pricing catalog changed during reconciliation; retrying latest snapshot"
                    );
                }
                result => return result,
            }
        }

        unreachable!("pricing sync attempts always return on their final iteration")
    }

    /// Share the parsed discovery catalog until the next refresh cycle.
    pub(crate) async fn metadata_snapshot(
        &self,
    ) -> Result<Arc<PricingCatalogSnapshot>, GatewayError> {
        let mut cached = self.metadata_snapshot.lock().await;
        if let Some(snapshot) = cached.as_ref() {
            return Ok(Arc::clone(snapshot));
        }
        let snapshot = Arc::new(self.load_snapshot_from_store_or_fallback().await?);
        *cached = Some(Arc::clone(&snapshot));
        Ok(snapshot)
    }

    pub(crate) async fn load_snapshot_from_store_or_fallback(
        &self,
    ) -> Result<PricingCatalogSnapshot, GatewayError> {
        if let Some(snapshot) = self.load_stored_snapshot(&self.catalog_key).await? {
            return Ok(snapshot);
        }
        // Read the old projection during an offline upgrade, but never reuse its
        // ETag for v3 refreshes or write a v2 document under the v3 key.
        if self.catalog_key == PRICING_CATALOG_CACHE_KEY
            && let Some(snapshot) = self
                .load_stored_snapshot(PREVIOUS_PRICING_CATALOG_CACHE_KEY)
                .await?
        {
            return Ok(snapshot);
        }
        Ok(self.fallback_snapshot.clone())
    }

    async fn load_stored_snapshot(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogSnapshot>, GatewayError> {
        let Some(cache) = self.repo.get_pricing_catalog_cache(catalog_key).await? else {
            return Ok(None);
        };

        match serde_json::from_str::<PricingCatalogDocument>(&cache.snapshot_json) {
            Ok(document) => Ok(Some(PricingCatalogSnapshot {
                metadata: PricingCatalogSnapshotMetadata {
                    checksum: Some(document_checksum(&cache.snapshot_json)),
                    source: cache.source,
                    etag: cache.etag,
                    fetched_at: cache.fetched_at,
                },
                document,
            })),
            Err(error) => {
                warn!(
                    catalog_key = %catalog_key,
                    error = %error,
                    "stored pricing catalog cache is invalid; trying fallback snapshot"
                );
                Ok(None)
            }
        }
    }

    async fn load_stored_cache(&self) -> Result<Option<PricingCatalogCacheRecord>, GatewayError> {
        Ok(self
            .repo
            .get_pricing_catalog_cache(&self.catalog_key)
            .await?)
    }

    async fn sync_model_pricing_snapshot(
        &self,
        snapshot: &PricingCatalogSnapshot,
    ) -> Result<(), GatewayError> {
        let active_rows = self.repo.list_active_model_pricing().await?;
        if snapshot_is_already_synced(&active_rows, snapshot) {
            return Ok(());
        }
        if active_rows
            .iter()
            .any(|row| row.provenance.fetched_at > snapshot.metadata.fetched_at)
        {
            return Err(GatewayError::Store(StoreError::PricingSyncConflict));
        }

        let active_by_target = active_rows
            .into_iter()
            .map(|record| {
                (
                    (
                        record.pricing_provider_id.clone(),
                        record.pricing_model_id.clone(),
                    ),
                    record,
                )
            })
            .collect::<BTreeMap<_, _>>();

        let mut snapshot_keys = BTreeSet::new();
        let mut changes = ModelPricingSyncChanges::default();
        for (pricing_provider_id, provider_document) in &snapshot.document.providers {
            for (pricing_model_id, model_document) in &provider_document.models {
                let desired = build_model_pricing_record(
                    &snapshot.metadata,
                    pricing_provider_id,
                    pricing_model_id,
                    model_document,
                )?;
                let key = (pricing_provider_id.clone(), pricing_model_id.clone());
                snapshot_keys.insert(key.clone());

                match active_by_target.get(&key) {
                    Some(existing)
                        if existing.provenance.fetched_at == snapshot.metadata.fetched_at =>
                    {
                        if existing.provenance != desired.provenance
                            || !pricing_record_matches(existing, &desired)
                        {
                            return Err(GatewayError::Store(StoreError::PricingSyncConflict));
                        }
                    }
                    Some(existing) if pricing_record_matches(existing, &desired) => {
                        if existing.provenance != desired.provenance {
                            changes
                                .update_provenance
                                .push(ModelPricingProvenanceUpdate {
                                    model_pricing_id: existing.model_pricing_id,
                                    provenance: desired.provenance,
                                    updated_at: snapshot.metadata.fetched_at,
                                });
                        }
                    }
                    Some(existing) => {
                        changes
                            .close_model_pricing_ids
                            .push(existing.model_pricing_id);
                        changes.insert_model_pricing.push(desired);
                    }
                    None => {
                        changes.insert_model_pricing.push(desired);
                    }
                }
            }
        }

        for (key, existing) in &active_by_target {
            if !snapshot_keys.contains(key) {
                if existing.provenance.fetched_at == snapshot.metadata.fetched_at {
                    return Err(GatewayError::Store(StoreError::PricingSyncConflict));
                }
                changes
                    .close_model_pricing_ids
                    .push(existing.model_pricing_id);
            }
        }

        if changes.close_model_pricing_ids.is_empty()
            && changes.update_provenance.is_empty()
            && changes.insert_model_pricing.is_empty()
        {
            return Ok(());
        }

        self.repo
            .apply_model_pricing_sync(&changes, snapshot.metadata.fetched_at)
            .await?;
        Ok(())
    }
}

/// Resolve the effective catalog pricing row for a target, preferring the exact id over
/// its [`catalog_model_id_fallback`] in a single repository lookup.
pub(crate) async fn resolve_catalog_pricing_record<R>(
    repo: &R,
    pricing_provider_id: &str,
    pricing_model_id: &str,
    occurred_at: OffsetDateTime,
) -> Result<Option<ModelPricingRecord>, GatewayError>
where
    R: PricingCatalogRepository + Send + Sync + ?Sized,
{
    let record = match catalog_model_id_fallback(pricing_provider_id, pricing_model_id) {
        Some(fallback_model_id) => {
            repo.resolve_model_pricing_with_fallback_at(
                pricing_provider_id,
                pricing_model_id,
                &fallback_model_id,
                occurred_at,
            )
            .await?
        }
        None => {
            repo.resolve_model_pricing_at(pricing_provider_id, pricing_model_id, occurred_at)
                .await?
        }
    };
    Ok(record)
}

pub async fn fetch_vendored_snapshot(
    source_url: &str,
) -> anyhow::Result<PricingCatalogSnapshotFile> {
    let request = catalog_http_client().get(source_url);
    let fetched = fetch_bounded(request, MAX_MODELS_DEV_RESPONSE_BYTES)
        .await
        .with_context(|| format!("failed fetching pricing catalog from `{source_url}`"))?;
    // No `If-None-Match` is sent, so a 304 is a misbehaving server.
    let CatalogFetch::Fetched(body) = fetched else {
        anyhow::bail!("pricing catalog fetch returned HTTP 304 to an unconditional request");
    };
    let ProjectedSnapshot { snapshot, .. } = project_models_dev_snapshot(
        &body.bytes,
        VENDORED_SOURCE,
        body.etag,
        OffsetDateTime::now_utc(),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(PricingCatalogSnapshotFile {
        metadata: snapshot.metadata,
        providers: snapshot.document.providers,
    })
}

/// Whether the vendored snapshot file `existing_json` holds the same projected providers
/// document as `fetched`, so a sync can leave the file, metadata included, untouched. A
/// file that does not parse never matches.
#[must_use]
pub fn vendored_snapshot_is_unchanged(
    existing_json: &str,
    fetched: &PricingCatalogSnapshotFile,
) -> bool {
    let Ok(existing) = serde_json::from_str::<PricingCatalogSnapshotFile>(existing_json) else {
        return false;
    };
    // Re-serialize rather than trust the file's own checksum, which an older sync may
    // not have written.
    let Ok(existing_document_json) = canonical_document_json(&existing.providers) else {
        return false;
    };
    fetched.metadata.checksum.as_deref()
        == Some(document_checksum(&existing_document_json).as_str())
}

pub fn snapshot_to_pretty_json(snapshot: &PricingCatalogSnapshotFile) -> anyhow::Result<String> {
    let mut json = serde_json::to_string_pretty(snapshot)
        .context("failed serializing vendored pricing catalog")?;
    // The repository's end-of-file-fixer hook rejects a commit of the file without it.
    json.push('\n');
    Ok(json)
}

fn cached_document_is_valid(cache: &PricingCatalogCacheRecord) -> bool {
    serde_json::from_str::<PricingCatalogDocument>(&cache.snapshot_json).is_ok()
}

fn next_catalog_generation_at(
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

fn resolved_model_pricing(record: &ModelPricingRecord) -> ResolvedModelPricing {
    ResolvedModelPricing {
        model_pricing_id: record.model_pricing_id,
        pricing_provider_id: record.pricing_provider_id.clone(),
        model_id: record.pricing_model_id.clone(),
        display_name: record.display_name.clone(),
        input_cost_per_million_tokens: record.input_cost_per_million_tokens,
        output_cost_per_million_tokens: record.output_cost_per_million_tokens,
        cache_read_cost_per_million_tokens: record.cache_read_cost_per_million_tokens,
        cache_write_cost_per_million_tokens: record.cache_write_cost_per_million_tokens,
        input_audio_cost_per_million_tokens: record.input_audio_cost_per_million_tokens,
        output_audio_cost_per_million_tokens: record.output_audio_cost_per_million_tokens,
        output_image_usd_per_million: record.output_image_usd_per_million,
        context_tiers: record.context_tiers.clone(),
        release_date: record.release_date.clone(),
        last_updated: record.last_updated.clone(),
        effective_start_at: record.effective_start_at,
        effective_end_at: record.effective_end_at,
        limits: record.limits.clone(),
        modalities: record.modalities.clone(),
        provenance: record.provenance.clone(),
    }
}

pub(crate) fn build_model_pricing_record(
    metadata: &PricingCatalogSnapshotMetadata,
    pricing_provider_id: &str,
    pricing_model_id: &str,
    document: &PricingCatalogModelDocument,
) -> Result<ModelPricingRecord, GatewayError> {
    Ok(ModelPricingRecord {
        model_pricing_id: Uuid::new_v4(),
        pricing_provider_id: pricing_provider_id.to_string(),
        pricing_model_id: pricing_model_id.to_string(),
        display_name: document.display_name.clone(),
        input_cost_per_million_tokens: parse_money(document.cost.input.as_deref())?,
        output_cost_per_million_tokens: parse_money(document.cost.output.as_deref())?,
        cache_read_cost_per_million_tokens: parse_money(document.cost.cache_read.as_deref())?,
        cache_write_cost_per_million_tokens: parse_money(document.cost.cache_write.as_deref())?,
        input_audio_cost_per_million_tokens: parse_money(document.cost.input_audio.as_deref())?,
        output_audio_cost_per_million_tokens: parse_money(document.cost.output_audio.as_deref())?,
        output_image_usd_per_million: None,
        context_tiers: Vec::new(),
        release_date: document.release_date.clone(),
        last_updated: document.last_updated.clone(),
        effective_start_at: metadata.fetched_at,
        effective_end_at: None,
        limits: PricingLimits {
            context: document.limit.context,
            input: document.limit.input,
            output: document.limit.output,
        },
        modalities: PricingModalities {
            input: document.modalities.input.clone(),
            output: document.modalities.output.clone(),
        },
        provenance: PricingProvenance {
            source: metadata.source.clone(),
            etag: metadata.etag.clone(),
            fetched_at: metadata.fetched_at,
        },
        created_at: metadata.fetched_at,
        updated_at: metadata.fetched_at,
    })
}

fn pricing_record_matches(existing: &ModelPricingRecord, desired: &ModelPricingRecord) -> bool {
    existing.display_name == desired.display_name
        && existing.input_cost_per_million_tokens == desired.input_cost_per_million_tokens
        && existing.output_cost_per_million_tokens == desired.output_cost_per_million_tokens
        && existing.cache_read_cost_per_million_tokens == desired.cache_read_cost_per_million_tokens
        && existing.cache_write_cost_per_million_tokens
            == desired.cache_write_cost_per_million_tokens
        && existing.input_audio_cost_per_million_tokens
            == desired.input_audio_cost_per_million_tokens
        && existing.output_audio_cost_per_million_tokens
            == desired.output_audio_cost_per_million_tokens
        && existing.output_image_usd_per_million == desired.output_image_usd_per_million
        && existing.context_tiers == desired.context_tiers
        && existing.release_date == desired.release_date
        && existing.last_updated == desired.last_updated
        && existing.limits == desired.limits
        && existing.modalities == desired.modalities
}

fn snapshot_is_already_synced(
    active_rows: &[ModelPricingRecord],
    snapshot: &PricingCatalogSnapshot,
) -> bool {
    let snapshot_keys = snapshot
        .document
        .providers
        .iter()
        .flat_map(|(provider_id, provider)| {
            provider
                .models
                .keys()
                .map(move |model_id| (provider_id.clone(), model_id.clone()))
        })
        .collect::<BTreeSet<_>>();

    !snapshot_keys.is_empty()
        && active_rows.len() == snapshot_keys.len()
        && active_rows.iter().all(|row| {
            row.provenance.source == snapshot.metadata.source
                && row.provenance.etag == snapshot.metadata.etag
                && row.provenance.fetched_at == snapshot.metadata.fetched_at
                && snapshot_keys.contains(&(
                    row.pricing_provider_id.clone(),
                    row.pricing_model_id.clone(),
                ))
        })
}

fn parse_money(value: Option<&str>) -> Result<Option<Money4>, GatewayError> {
    value
        .map(|raw| {
            Money4::from_decimal_str(raw).map_err(|error| {
                GatewayError::Internal(format!(
                    "invalid pricing catalog money value `{raw}`: {error}"
                ))
            })
        })
        .transpose()
}

fn project_models_dev_snapshot(
    body: &[u8],
    source: &str,
    etag: Option<String>,
    fetched_at: OffsetDateTime,
) -> Result<ProjectedSnapshot, GatewayError> {
    let providers = serde_json::from_slice::<BTreeMap<String, ModelsDevProviderDocument>>(body)
        .map_err(|error| {
            GatewayError::Internal(format!("failed parsing models.dev response: {error}"))
        })?;

    let mut projected_providers = BTreeMap::new();
    for supported_provider_id in SUPPORTED_PRICING_PROVIDER_IDS {
        let Some(provider) = providers.get(supported_provider_id) else {
            continue;
        };

        let mut projected_models = BTreeMap::new();
        for (fallback_key, model) in &provider.models {
            let model_id = if model.id.trim().is_empty() {
                fallback_key.clone()
            } else {
                model.id.clone()
            };
            projected_models.insert(
                model_id.clone(),
                PricingCatalogModelDocument {
                    id: model_id,
                    display_name: model.name.clone(),
                    release_date: model.release_date.clone(),
                    last_updated: model.last_updated.clone(),
                    metadata: model.metadata.clone(),
                    cost: PricingCatalogCostDocument {
                        input: project_models_dev_cost(model.cost.input.as_ref())?,
                        output: project_models_dev_cost(model.cost.output.as_ref())?,
                        cache_read: project_models_dev_cost(model.cost.cache_read.as_ref())?,
                        cache_write: project_models_dev_cost(model.cost.cache_write.as_ref())?,
                        input_audio: project_models_dev_cost(model.cost.input_audio.as_ref())?,
                        output_audio: project_models_dev_cost(model.cost.output_audio.as_ref())?,
                        conditions: model.cost.conditions.clone(),
                    },
                    limit: PricingCatalogLimitDocument {
                        context: model.limit.context,
                        input: model.limit.input,
                        output: model.limit.output,
                    },
                    modalities: PricingCatalogModalitiesDocument {
                        input: model.modalities.input.clone(),
                        output: model.modalities.output.clone(),
                    },
                },
            );
        }

        projected_providers.insert(
            supported_provider_id.to_string(),
            PricingCatalogProviderDocument {
                display_name: provider.name.clone(),
                models: projected_models,
            },
        );
    }

    projected_snapshot(projected_providers, source, etag, fetched_at)
}

/// A projected snapshot and the canonical document JSON its checksum covers, so a cache
/// write stores those bytes without serializing the document again.
struct ProjectedSnapshot {
    snapshot: PricingCatalogSnapshot,
    document_json: String,
}

/// Stamp a projected providers document with its metadata and checksum.
fn projected_snapshot(
    providers: BTreeMap<String, PricingCatalogProviderDocument>,
    source: &str,
    etag: Option<String>,
    fetched_at: OffsetDateTime,
) -> Result<ProjectedSnapshot, GatewayError> {
    let document_json = canonical_document_json(&providers).map_err(|error| {
        GatewayError::Internal(format!(
            "failed serializing pricing catalog snapshot: {error}"
        ))
    })?;
    Ok(ProjectedSnapshot {
        snapshot: PricingCatalogSnapshot {
            metadata: PricingCatalogSnapshotMetadata {
                source: source.to_string(),
                etag,
                fetched_at,
                checksum: Some(document_checksum(&document_json)),
            },
            document: PricingCatalogDocument { providers },
        },
        document_json,
    })
}

/// The canonical JSON of a projected providers document: pretty-printed, exactly as
/// [`PricingCatalogDocument`] serializes, and stored as the cache row's `snapshot_json`.
fn canonical_document_json(
    providers: &BTreeMap<String, PricingCatalogProviderDocument>,
) -> serde_json::Result<String> {
    /// Borrowing twin of [`PricingCatalogDocument`], so serializing needs no clone.
    #[derive(Serialize)]
    struct DocumentRef<'doc> {
        providers: &'doc BTreeMap<String, PricingCatalogProviderDocument>,
    }
    serde_json::to_string_pretty(&DocumentRef { providers })
}

/// SHA-256, as lowercase hex, of [`canonical_document_json`] output.
#[must_use]
fn document_checksum(document_json: &str) -> String {
    format!("{:x}", Sha256::digest(document_json.as_bytes()))
}

fn project_models_dev_cost(value: Option<&Number>) -> Result<Option<String>, GatewayError> {
    value.map(normalize_models_dev_money).transpose()
}

fn normalize_models_dev_money(number: &Number) -> Result<String, GatewayError> {
    let raw = number.to_string();
    if let Ok(money) = Money4::from_decimal_str(&raw) {
        return Ok(money.format_4dp());
    }

    let value = number.as_f64().ok_or_else(|| {
        GatewayError::Internal(format!(
            "failed normalizing models.dev cost `{raw}`: not finite"
        ))
    })?;
    let scaled = (value * Money4::SCALE as f64).round();
    if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(GatewayError::Internal(format!(
            "failed normalizing models.dev cost `{raw}`: rounded value overflowed"
        )));
    }

    Ok(Money4::from_scaled(scaled as i64).format_4dp())
}

pub(crate) fn load_vendored_fallback_snapshot() -> PricingCatalogSnapshot {
    let snapshot = serde_json::from_str::<PricingCatalogSnapshotFile>(VENDORED_FALLBACK_JSON)
        .expect("vendored pricing catalog fallback should deserialize");
    PricingCatalogSnapshot {
        metadata: snapshot.metadata,
        document: PricingCatalogDocument {
            providers: snapshot.providers,
        },
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PricingCatalogSnapshot {
    pub(crate) metadata: PricingCatalogSnapshotMetadata,
    pub(crate) document: PricingCatalogDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogSnapshotFile {
    pub metadata: PricingCatalogSnapshotMetadata,
    pub providers: BTreeMap<String, PricingCatalogProviderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogSnapshotMetadata {
    pub source: String,
    pub etag: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub fetched_at: OffsetDateTime,
    /// SHA-256 (lowercase hex) of the projected providers document's canonical JSON; the
    /// metadata itself is not covered, so a refetch of unchanged content keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PricingCatalogDocument {
    pub(crate) providers: BTreeMap<String, PricingCatalogProviderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogProviderDocument {
    pub display_name: String,
    pub models: BTreeMap<String, PricingCatalogModelDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogModelDocument {
    pub id: String,
    pub display_name: String,
    pub release_date: String,
    pub last_updated: String,
    #[serde(default)]
    pub metadata: metadata::CatalogModelMetadata,
    pub cost: PricingCatalogCostDocument,
    pub limit: PricingCatalogLimitDocument,
    pub modalities: PricingCatalogModalitiesDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingCatalogCostDocument {
    /// Source-shaped conditional rates, in USD per million tokens; informational only.
    #[serde(default)]
    pub conditions: BTreeMap<String, serde_json::Value>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub cache_read: Option<String>,
    pub cache_write: Option<String>,
    pub input_audio: Option<String>,
    pub output_audio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingCatalogLimitDocument {
    pub context: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingCatalogModalitiesDocument {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelsDevProviderDocument {
    name: String,
    #[serde(default)]
    models: BTreeMap<String, ModelsDevModelDocument>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelsDevModelDocument {
    #[serde(flatten)]
    metadata: metadata::CatalogModelMetadata,
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    last_updated: String,
    #[serde(default)]
    cost: ModelsDevCostDocument,
    #[serde(default)]
    limit: ModelsDevLimitDocument,
    #[serde(default)]
    modalities: ModelsDevModalitiesDocument,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsDevCostDocument {
    #[serde(flatten)]
    conditions: BTreeMap<String, serde_json::Value>,
    input: Option<Number>,
    output: Option<Number>,
    cache_read: Option<Number>,
    cache_write: Option<Number>,
    input_audio: Option<Number>,
    output_audio: Option<Number>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsDevLimitDocument {
    context: Option<i64>,
    input: Option<i64>,
    output: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsDevModalitiesDocument {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

#[cfg(test)]
mod tests;
