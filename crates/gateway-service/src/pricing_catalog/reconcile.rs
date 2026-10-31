//! Reconciles the merged catalog snapshot into `model_pricing` as one generation.
//!
//! Rows take effect at the snapshot's generation (its newest source `fetched_at`); each
//! row's provenance is the snapshot of the source that supplied its pricing.

use std::collections::BTreeMap;

use gateway_core::{
    ContextPricingTier, GatewayError, ModelPricingProvenanceUpdate, ModelPricingRecord,
    ModelPricingSyncChanges, Money4, PricingCatalogRepository, PricingLimits, PricingModalities,
    PricingProvenance, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    PricingCatalogContextTier, PricingCatalogModelDocument, PricingCatalogSnapshot,
    PricingCatalogSnapshotMetadata, merge::MergedModelReport, sources::CatalogSourceId,
};

type PricingTargetKey = (String, String);

/// Close, re-stamp and insert `model_pricing` rows so the active rows match `snapshot`.
///
/// # Errors
///
/// Returns [`StoreError::PricingSyncConflict`] when active rows belong to a newer
/// generation or this generation was applied differently, so the caller reloads.
pub(super) async fn reconcile_model_pricing<R>(
    repo: &R,
    snapshot: &PricingCatalogSnapshot,
) -> Result<(), GatewayError>
where
    R: PricingCatalogRepository + Send + Sync + ?Sized,
{
    let generation = snapshot.generation_at();
    let desired_rows = desired_pricing_rows(snapshot, generation)?;
    let active_rows = repo.list_active_model_pricing().await?;
    if active_rows
        .iter()
        .any(|row| row.provenance.fetched_at > generation)
    {
        return Err(StoreError::PricingSyncConflict.into());
    }

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
            // Already applied in this generation: a row of this generation cannot be
            // replaced, so any difference is a concurrent reconciliation.
            Some(existing) if existing.provenance.fetched_at == generation => {
                if existing.provenance != desired.provenance
                    || !pricing_record_matches(&existing, &desired)
                {
                    return Err(StoreError::PricingSyncConflict.into());
                }
            }
            Some(existing) if pricing_record_matches(&existing, &desired) => {
                if existing.provenance != desired.provenance {
                    changes
                        .update_provenance
                        .push(ModelPricingProvenanceUpdate {
                            model_pricing_id: existing.model_pricing_id,
                            provenance: desired.provenance,
                            updated_at: generation,
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
        if existing.provenance.fetched_at == generation {
            return Err(StoreError::PricingSyncConflict.into());
        }
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
    repo.apply_model_pricing_sync(&changes, generation).await?;
    Ok(())
}

/// One row per merged model that some source attributes data to, keyed by target.
fn desired_pricing_rows(
    snapshot: &PricingCatalogSnapshot,
    generation: OffsetDateTime,
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
                generation,
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

/// A pricing row stamped with `provenance`, effective at `generation`.
fn pricing_record_at(
    provenance: &PricingCatalogSnapshotMetadata,
    generation: OffsetDateTime,
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
        output_image_usd_per_million: parse_money(document.cost.output_image.as_deref())?,
        context_tiers: document
            .cost
            .tiers
            .iter()
            .map(context_pricing_tier)
            .collect::<Result<_, _>>()?,
        release_date: document.release_date.clone(),
        last_updated: document.last_updated.clone(),
        effective_start_at: generation,
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
        created_at: generation,
        updated_at: generation,
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
