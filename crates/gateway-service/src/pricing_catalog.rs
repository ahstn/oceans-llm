use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::Context;
use gateway_core::{
    GatewayError, ModelPricingRecord, ModelRoute, PricingCatalogRepository, PricingResolution,
    PricingUnpricedReason, ProviderConnection, ResolvedModelPricing, StoreError,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tracing::warn;

use crate::{admin_audit::AuditProgress, catalog_fetch::catalog_http_client};

pub const DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_PRICING_SYNC_ATTEMPTS: usize = 3;

pub(crate) mod merge;
pub mod metadata;
mod reconcile;
mod refresh;
pub(crate) mod sources;
mod target;

use merge::{MergedModelReport, merge_catalogs};
#[cfg(test)]
pub(crate) use reconcile::build_model_pricing_record;
use refresh::{CatalogSource, RefreshMode};
pub use refresh::{
    PricingCatalogRefreshError, PricingCatalogRefreshOutcome, PricingCatalogSourceRefresh,
};
use sources::{CatalogSourceId, Providers};
#[cfg(test)]
use target::normalize_bedrock_pricing_model_id;
use target::{PricingTarget, pricing_target_for_route};
pub use target::{SUPPORTED_PRICING_PROVIDER_IDS, is_supported_pricing_provider_id};
pub(crate) use target::{
    catalog_metadata_target_for_route, catalog_model_id_fallback,
    catalog_pricing_supported_for_route,
};

/// The pricing and discovery catalog merged from models.dev, OpenRouter and LiteLLM.
///
/// Each source is refreshed and cached on its own; the merged snapshot is rebuilt when a
/// source's snapshot changes and reconciled into `model_pricing` as one generation.
#[derive(Clone)]
pub struct PricingCatalog<R> {
    repo: Arc<R>,
    client: Client,
    refresh_interval: Duration,
    /// Every source, in [`CatalogSourceId::ALL`] order.
    sources: Arc<[CatalogSource; 3]>,
    snapshot: Arc<tokio::sync::Mutex<Option<Arc<PricingCatalogSnapshot>>>>,
}

impl<R> PricingCatalog<R>
where
    R: PricingCatalogRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self::with_sources(
            repo,
            DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL,
            CatalogSourceId::ALL.map(CatalogSource::vendored),
        )
    }

    /// `sources` must be in [`CatalogSourceId::ALL`] order.
    fn with_sources(repo: Arc<R>, refresh_interval: Duration, sources: [CatalogSource; 3]) -> Self {
        Self {
            repo,
            client: catalog_http_client(),
            refresh_interval,
            sources: Arc::new(sources),
            snapshot: Default::default(),
        }
    }

    /// Refresh the stale sources, then reconcile pricing from the current snapshots. A
    /// source that fails to refresh keeps its snapshot and is retried next time.
    ///
    /// # Errors
    ///
    /// Fails when the snapshots cannot be loaded or reconciliation fails.
    pub async fn refresh_if_stale_and_sync(&self) -> Result<(), GatewayError> {
        self.refresh_sources(RefreshMode::IfStale).await;
        self.sync_latest_snapshot_with_retry().await?;
        Ok(())
    }

    /// Refresh every source unconditionally, then reconcile pricing. `progress` is
    /// marked once a source's snapshot is cached, so an audit entry can record a refresh
    /// whose reconciliation fails.
    ///
    /// Returns one result per source, in [`CatalogSourceId::ALL`] order; a failed source
    /// keeps its cached or vendored snapshot.
    ///
    /// # Errors
    ///
    /// Fails only when the snapshots cannot be loaded or reconciliation fails.
    pub async fn refresh_now_and_sync_tracked(
        &self,
        progress: &AuditProgress,
    ) -> Result<Vec<PricingCatalogSourceRefresh>, GatewayError> {
        let outcomes = self.refresh_sources(RefreshMode::Forced).await;
        if outcomes
            .iter()
            .any(|(_, outcome)| *outcome == PricingCatalogRefreshOutcome::Updated)
        {
            progress.landed();
        }
        let snapshot = self.sync_latest_snapshot_with_retry().await?;
        Ok(outcomes
            .into_iter()
            .map(|(source, outcome)| PricingCatalogSourceRefresh {
                source,
                outcome,
                fetched_at: snapshot.sources[&source].fetched_at,
            })
            .collect())
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

    /// Reconcile the latest merged snapshot, reloading and retrying when another
    /// reconciliation raced this one. Returns the reconciled snapshot.
    async fn sync_latest_snapshot_with_retry(
        &self,
    ) -> Result<Arc<PricingCatalogSnapshot>, GatewayError> {
        for attempt in 1..=MAX_PRICING_SYNC_ATTEMPTS {
            // Re-read on each attempt, including changes from other replicas.
            let snapshot = self.reload_snapshot().await?;
            match reconcile::reconcile_model_pricing(self.repo.as_ref(), &snapshot).await {
                Err(GatewayError::Store(StoreError::PricingSyncConflict))
                    if attempt < MAX_PRICING_SYNC_ATTEMPTS =>
                {
                    warn!(
                        attempt,
                        "pricing catalog changed during reconciliation; retrying latest snapshot"
                    );
                }
                result => return result.map(|()| snapshot),
            }
        }

        unreachable!("pricing sync attempts always return on their final iteration")
    }

    /// Share the merged catalog until a source's snapshot changes.
    pub(crate) async fn metadata_snapshot(
        &self,
    ) -> Result<Arc<PricingCatalogSnapshot>, GatewayError> {
        let mut cached = self.snapshot.lock().await;
        if let Some(snapshot) = cached.as_ref() {
            return Ok(Arc::clone(snapshot));
        }
        let snapshot = Arc::new(PricingCatalogSnapshot::from_sources(
            self.load_source_snapshots().await?,
        ));
        *cached = Some(Arc::clone(&snapshot));
        Ok(snapshot)
    }

    /// Load every source's snapshot and replace the shared merged snapshot only when one
    /// of them differs from the snapshot it was merged from.
    async fn reload_snapshot(&self) -> Result<Arc<PricingCatalogSnapshot>, GatewayError> {
        let mut cached = self.snapshot.lock().await;
        let sources = self.load_source_snapshots().await?;
        if let Some(current) = cached.as_ref()
            && current.is_merged_from(&sources)
        {
            return Ok(Arc::clone(current));
        }
        let snapshot = Arc::new(PricingCatalogSnapshot::from_sources(sources));
        *cached = Some(Arc::clone(&snapshot));
        Ok(snapshot)
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

/// A projected source snapshot and the canonical document JSON its checksum covers, so a
/// cache write stores those bytes without serializing the document again.
struct ProjectedSnapshot {
    snapshot: PricingCatalogSnapshotFile,
    document_json: String,
}

/// Stamp a projected providers document with its metadata and checksum.
fn projected_snapshot(
    providers: Providers,
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
        snapshot: PricingCatalogSnapshotFile {
            metadata: PricingCatalogSnapshotMetadata {
                source: source.to_string(),
                etag,
                fetched_at,
                checksum: Some(document_checksum(&document_json)),
            },
            providers,
        },
        document_json,
    })
}

/// The canonical JSON of a projected providers document: pretty-printed, exactly as
/// [`PricingCatalogDocument`] serializes, and stored as the cache row's `snapshot_json`.
fn canonical_document_json(providers: &Providers) -> serde_json::Result<String> {
    /// Borrowing twin of [`PricingCatalogDocument`], so serializing needs no clone.
    #[derive(Serialize)]
    struct DocumentRef<'doc> {
        providers: &'doc Providers,
    }
    serde_json::to_string_pretty(&DocumentRef { providers })
}

/// SHA-256, as lowercase hex, of [`canonical_document_json`] output.
#[must_use]
fn document_checksum(document_json: &str) -> String {
    format!("{:x}", Sha256::digest(document_json.as_bytes()))
}

/// The merged snapshot of the three checked-in source snapshots.
#[cfg(test)]
pub(crate) fn load_vendored_fallback_snapshot() -> PricingCatalogSnapshot {
    PricingCatalogSnapshot::from_sources(
        CatalogSourceId::ALL
            .into_iter()
            .map(|source| (source, source.descriptor().vendored_snapshot()))
            .collect(),
    )
}

/// The merged catalog: precedence applied per field, with per-source provenance.
#[derive(Debug, Clone)]
pub(crate) struct PricingCatalogSnapshot {
    /// Merged providers document (precedence applied).
    pub(crate) document: PricingCatalogDocument,
    /// Provenance of the snapshot each source contributed (vendored or remote). Always
    /// has all three keys.
    pub(crate) sources: BTreeMap<CatalogSourceId, PricingCatalogSnapshotMetadata>,
    /// Merge provenance and conflicts keyed by (provider_id, model_id).
    pub(crate) reports: BTreeMap<(String, String), MergedModelReport>,
}

impl PricingCatalogSnapshot {
    /// Merge one snapshot per source; `sources` must hold every [`CatalogSourceId`].
    pub(crate) fn from_sources(
        sources: BTreeMap<CatalogSourceId, PricingCatalogSnapshotFile>,
    ) -> Self {
        let empty = Providers::new();
        let providers =
            |source: CatalogSourceId| sources.get(&source).map_or(&empty, |file| &file.providers);
        let merged = merge_catalogs(
            providers(CatalogSourceId::ModelsDev),
            providers(CatalogSourceId::OpenRouter),
            providers(CatalogSourceId::LiteLlm),
        );
        Self {
            document: PricingCatalogDocument {
                providers: merged.providers,
            },
            sources: sources
                .into_iter()
                .map(|(source, file)| (source, file.metadata))
                .collect(),
            reports: merged.reports,
        }
    }

    /// The merged model at exactly `provider_id` / `model_id`.
    pub(crate) fn model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Option<&PricingCatalogModelDocument> {
        self.document
            .providers
            .get(provider_id)?
            .models
            .get(model_id)
    }

    /// The merge report of the model at exactly `provider_id` / `model_id`.
    pub(crate) fn report(&self, provider_id: &str, model_id: &str) -> Option<&MergedModelReport> {
        self.reports
            .get(&(provider_id.to_string(), model_id.to_string()))
    }

    /// The newest source `fetched_at`: pricing rows reconciled from this snapshot take
    /// effect then.
    ///
    /// # Panics
    ///
    /// Panics when the snapshot has no sources, which [`Self::from_sources`] rules out.
    pub(crate) fn generation_at(&self) -> OffsetDateTime {
        self.sources
            .values()
            .map(|metadata| metadata.fetched_at)
            .max()
            .expect("a merged snapshot has every source")
    }

    /// Whether this snapshot was merged from exactly `sources`' snapshot metadata,
    /// checksums included.
    fn is_merged_from(
        &self,
        sources: &BTreeMap<CatalogSourceId, PricingCatalogSnapshotFile>,
    ) -> bool {
        self.sources.len() == sources.len()
            && sources
                .iter()
                .all(|(source, file)| self.sources.get(source) == Some(&file.metadata))
    }
}

/// A source snapshot as vendored on disk and cached at runtime: provenance plus the
/// projected providers document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogSnapshotFile {
    pub metadata: PricingCatalogSnapshotMetadata,
    pub providers: BTreeMap<String, PricingCatalogProviderDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// A source's context tiers live in [`Self::tiers`], not here, once they are typed.
    #[serde(default)]
    pub conditions: BTreeMap<String, serde_json::Value>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub cache_read: Option<String>,
    pub cache_write: Option<String>,
    pub input_audio: Option<String>,
    pub output_audio: Option<String>,
    /// USD per million image output tokens.
    #[serde(default)]
    pub output_image: Option<String>,
    /// Sorted ascending by `above_input_tokens`, thresholds unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<PricingCatalogContextTier>,
}

/// Rates that replace the base schedule for a whole request above a context size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PricingCatalogContextTier {
    /// Tier applies when normalized prompt tokens (cache included) are strictly greater
    /// than this.
    pub above_input_tokens: i64,
    /// USD per million tokens, 4 dp, like the base fields; `None` inherits the base rate.
    pub input: Option<String>,
    pub output: Option<String>,
    pub cache_read: Option<String>,
    pub cache_write: Option<String>,
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

#[cfg(test)]
mod tests;
