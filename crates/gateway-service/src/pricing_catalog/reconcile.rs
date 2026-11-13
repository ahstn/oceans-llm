//! Reconciles the merged catalog snapshot into `model_pricing` as one generation.
//!
//! Rows take effect at the snapshot's generation (its newest source `fetched_at`), or
//! after every active row's last sync when that is later; each row's provenance is the
//! snapshot of the source that supplied its pricing. Two combinations of source snapshots
//! can share a generation, so the store accepts a sync only while the catalog cache rows
//! still hold the snapshot's sources.

use std::collections::BTreeMap;

use gateway_core::{
    ContextPricingTier, GatewayError, ModelPricingProvenanceUpdate, ModelPricingRecord,
    ModelPricingSyncChanges, Money4, PricingCatalogRepository, PricingLimits, PricingModalities,
    PricingProvenance, ServiceTier, ServiceTierPricing,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    PricingCatalogContextTier, PricingCatalogDocument, PricingCatalogModelDocument,
    PricingCatalogServiceTierCost, PricingCatalogSnapshot, PricingCatalogSnapshotMetadata,
    merge::MergedModelReport, sources::CatalogSourceId,
};

type PricingTargetKey = (String, String);

/// Close, re-stamp and insert `model_pricing` rows so the active rows match `snapshot`.
///
/// # Errors
///
/// Returns [`gateway_core::StoreError::PricingSyncConflict`] when a catalog cache row no
/// longer holds the snapshot's source or another sync changed the active rows, so the
/// caller reloads.
pub(super) async fn reconcile_model_pricing<R>(
    repo: &R,
    snapshot: &PricingCatalogSnapshot,
) -> Result<(), GatewayError>
where
    R: PricingCatalogRepository + Send + Sync + ?Sized,
{
    let active_rows = repo.list_active_model_pricing().await?;
    let effective_at = sync_effective_at(snapshot.generation_at(), &active_rows);
    let desired_rows = desired_pricing_rows(snapshot, effective_at)?;

    let mut active_by_target = active_rows
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
    let mut changes = ModelPricingSyncChanges::default();
    for (key, desired) in desired_rows {
        match active_by_target.remove(&key) {
            Some(existing) if pricing_record_matches(&existing, &desired) => {
                if existing.provenance != desired.provenance {
                    changes
                        .update_provenance
                        .push(ModelPricingProvenanceUpdate {
                            model_pricing_id: existing.model_pricing_id,
                            provenance: desired.provenance,
                            updated_at: effective_at,
                        });
                }
            }
            Some(existing) => {
                changes
                    .close_model_pricing_ids
                    .push(existing.model_pricing_id);
                changes.insert_model_pricing.push(desired);
            }
            None => changes.insert_model_pricing.push(desired),
        }
    }
    // Rows the snapshot no longer lists.
    for existing in active_by_target.into_values() {
        changes
            .close_model_pricing_ids
            .push(existing.model_pricing_id);
    }

    if changes.close_model_pricing_ids.is_empty()
        && changes.update_provenance.is_empty()
        && changes.insert_model_pricing.is_empty()
    {
        return Ok(());
    }
    changes.catalog_cache_fetched_at = catalog_cache_revisions(repo, snapshot).await?;
    repo.apply_model_pricing_sync(&changes, effective_at)
        .await?;
    Ok(())
}

/// When a sync takes effect: the snapshot's `generation`, or one second after the newest
/// active row's last sync when that is not earlier. A newer combination of source
/// snapshots can share the generation of the one applied and still has to replace its
/// rows.
fn sync_effective_at(
    generation: OffsetDateTime,
    active_rows: &[ModelPricingRecord],
) -> OffsetDateTime {
    active_rows
        .iter()
        .map(|row| row.updated_at)
        .max()
        .filter(|newest| *newest >= generation)
        .map_or(generation, |newest| newest + Duration::seconds(1))
}

/// The catalog cache revision each source of `snapshot` stands for, keyed by
/// `catalog_key`: the row's `fetched_at` when the snapshot was loaded from it or the row
/// does not parse (so the source fell back to its vendored snapshot), else the snapshot's
/// own `fetched_at`, which the store rejects while a newer row exists.
async fn catalog_cache_revisions<R>(
    repo: &R,
    snapshot: &PricingCatalogSnapshot,
) -> Result<BTreeMap<String, OffsetDateTime>, GatewayError>
where
    R: PricingCatalogRepository + Send + Sync + ?Sized,
{
    let mut revisions = BTreeMap::new();
    for (source, metadata) in &snapshot.sources {
        let catalog_key = source.descriptor().cache_key;
        let fetched_at = match repo.get_pricing_catalog_cache(catalog_key).await? {
            Some(row)
                if row.fetched_at == metadata.fetched_at
                    || serde_json::from_str::<PricingCatalogDocument>(&row.snapshot_json)
                        .is_err() =>
            {
                row.fetched_at
            }
            _ => metadata.fetched_at,
        };
        revisions.insert(catalog_key.to_string(), fetched_at);
    }
    Ok(revisions)
}

/// One row per merged model that some source attributes data to, keyed by target.
fn desired_pricing_rows(
    snapshot: &PricingCatalogSnapshot,
    effective_at: OffsetDateTime,
) -> Result<BTreeMap<PricingTargetKey, ModelPricingRecord>, GatewayError> {
    let mut rows = BTreeMap::new();
    for (provider_id, provider) in &snapshot.document.providers {
        for (model_id, document) in &provider.models {
            let Some(source) = snapshot
                .report(provider_id, model_id)
                .and_then(row_provenance_source)
            else {
                continue;
            };
            let record = pricing_record_at(
                &snapshot.sources[&source],
                effective_at,
                provider_id,
                model_id,
                document,
            )?;
            rows.insert((provider_id.clone(), model_id.clone()), record);
        }
    }
    Ok(rows)
}

/// The source whose snapshot stamps a row: the pricing winner, else (for a model no
/// source prices) the source of its modalities or limits. `None` when no source
/// attributes a pricing-row field to the model.
fn row_provenance_source(report: &MergedModelReport) -> Option<CatalogSourceId> {
    let provenance = &report.provenance;
    provenance
        .pricing
        .or(provenance.modalities)
        .or(provenance.limits.context)
        .or(provenance.limits.input)
        .or(provenance.limits.output)
}

/// A pricing row from a single source snapshot, effective at its `fetched_at`.
#[cfg(test)]
pub(crate) fn build_model_pricing_record(
    metadata: &PricingCatalogSnapshotMetadata,
    pricing_provider_id: &str,
    pricing_model_id: &str,
    document: &PricingCatalogModelDocument,
) -> Result<ModelPricingRecord, GatewayError> {
    pricing_record_at(
        metadata,
        metadata.fetched_at,
        pricing_provider_id,
        pricing_model_id,
        document,
    )
}

/// A pricing row stamped with `provenance`, effective at `effective_at`.
fn pricing_record_at(
    provenance: &PricingCatalogSnapshotMetadata,
    effective_at: OffsetDateTime,
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
        cache_write_1h_cost_per_million_tokens: parse_money(
            document.cost.cache_write_1h.as_deref(),
        )?,
        input_audio_cost_per_million_tokens: parse_money(document.cost.input_audio.as_deref())?,
        output_audio_cost_per_million_tokens: parse_money(document.cost.output_audio.as_deref())?,
        output_image_usd_per_million: parse_money(document.cost.output_image.as_deref())?,
        context_tiers: document
            .cost
            .tiers
            .iter()
            .map(context_pricing_tier)
            .collect::<Result<_, _>>()?,
        service_tiers: document
            .cost
            .service_tiers
            .iter()
            .filter(|(tier, _)| **tier != ServiceTier::Default)
            .map(|(tier, cost)| service_tier_pricing(*tier, cost))
            .collect::<Result<_, _>>()?,
        release_date: document.release_date.clone(),
        last_updated: document.last_updated.clone(),
        effective_start_at: effective_at,
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
            source: provenance.source.clone(),
            etag: provenance.etag.clone(),
            fetched_at: provenance.fetched_at,
        },
        created_at: effective_at,
        updated_at: effective_at,
    })
}

fn context_pricing_tier(
    tier: &PricingCatalogContextTier,
) -> Result<ContextPricingTier, GatewayError> {
    Ok(ContextPricingTier {
        above_input_tokens: tier.above_input_tokens,
        input: parse_money(tier.input.as_deref())?,
        output: parse_money(tier.output.as_deref())?,
        cache_read: parse_money(tier.cache_read.as_deref())?,
        cache_write: parse_money(tier.cache_write.as_deref())?,
        cache_write_1h: parse_money(tier.cache_write_1h.as_deref())?,
    })
}

fn service_tier_pricing(
    service_tier: ServiceTier,
    cost: &PricingCatalogServiceTierCost,
) -> Result<ServiceTierPricing, GatewayError> {
    Ok(ServiceTierPricing {
        service_tier,
        input: parse_money(cost.input.as_deref())?,
        output: parse_money(cost.output.as_deref())?,
        cache_read: parse_money(cost.cache_read.as_deref())?,
        cache_write: parse_money(cost.cache_write.as_deref())?,
        cache_write_1h: parse_money(cost.cache_write_1h.as_deref())?,
        context_tiers: cost
            .context_tiers
            .iter()
            .map(context_pricing_tier)
            .collect::<Result<_, _>>()?,
    })
}

fn pricing_record_matches(existing: &ModelPricingRecord, desired: &ModelPricingRecord) -> bool {
    existing.display_name == desired.display_name
        && existing.input_cost_per_million_tokens == desired.input_cost_per_million_tokens
        && existing.output_cost_per_million_tokens == desired.output_cost_per_million_tokens
        && existing.cache_read_cost_per_million_tokens == desired.cache_read_cost_per_million_tokens
        && existing.cache_write_cost_per_million_tokens
            == desired.cache_write_cost_per_million_tokens
        && existing.cache_write_1h_cost_per_million_tokens
            == desired.cache_write_1h_cost_per_million_tokens
        && existing.input_audio_cost_per_million_tokens
            == desired.input_audio_cost_per_million_tokens
        && existing.output_audio_cost_per_million_tokens
            == desired.output_audio_cost_per_million_tokens
        && existing.output_image_usd_per_million == desired.output_image_usd_per_million
        && existing.context_tiers == desired.context_tiers
        && existing.service_tiers == desired.service_tiers
        && existing.release_date == desired.release_date
        && existing.last_updated == desired.last_updated
        && existing.limits == desired.limits
        && existing.modalities == desired.modalities
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
