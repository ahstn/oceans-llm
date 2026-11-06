use std::collections::{HashMap, HashSet};

use gateway_core::{ModelPricingSyncChanges, StoreError};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) struct ActiveModelPricing {
    pub(crate) model_pricing_id: Uuid,
    pub(crate) pricing_provider_id: String,
    pub(crate) pricing_model_id: String,
    /// The sync that last wrote the row: the one that inserted it or re-stamped its
    /// provenance.
    pub(crate) updated_at: OffsetDateTime,
}

/// The store state a pricing sync is validated against, read inside its transaction.
pub(crate) struct ModelPricingSyncState {
    pub(crate) active_rows: Vec<ActiveModelPricing>,
    /// The `fetched_at` of every `pricing_catalog_cache` row, keyed by `catalog_key`.
    pub(crate) catalog_cache_fetched_at: HashMap<String, OffsetDateTime>,
    /// The targets of every row, active or closed, that starts at the sync's
    /// `effective_at`.
    pub(crate) targets_starting_at_effective_at: HashSet<(String, String)>,
}

/// Accept `changes` only when they were computed from the latest catalog cache rows and
/// the current active rows, and only move pricing history forward.
///
/// Every rejection is [`StoreError::PricingSyncConflict`], so the caller reloads and
/// retries, except a duplicate target inside `changes`, which no reload can fix.
pub(crate) fn validate_model_pricing_sync(
    changes: &ModelPricingSyncChanges,
    effective_at: OffsetDateTime,
    state: ModelPricingSyncState,
) -> Result<(), StoreError> {
    // An absent row means the source used its vendored snapshot.
    if changes
        .catalog_cache_fetched_at
        .iter()
        .any(|(catalog_key, fetched_at)| {
            state
                .catalog_cache_fetched_at
                .get(catalog_key)
                .is_some_and(|stored| stored != fetched_at)
        })
    {
        return Err(StoreError::PricingSyncConflict);
    }

    let mut active_ids = HashSet::new();
    let mut active_by_target = HashMap::new();
    for row in state.active_rows {
        // Syncs only move forward, so a row written at or after `effective_at` belongs to
        // a sync this one did not see.
        if row.updated_at >= effective_at {
            return Err(StoreError::PricingSyncConflict);
        }
        active_ids.insert(row.model_pricing_id);
        active_by_target.insert(
            (row.pricing_provider_id, row.pricing_model_id),
            row.model_pricing_id,
        );
    }

    let close_ids = changes
        .close_model_pricing_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let expected_active_ids = close_ids.iter().copied().chain(
        changes
            .update_provenance
            .iter()
            .map(|update| update.model_pricing_id),
    );
    if expected_active_ids
        .into_iter()
        .any(|model_pricing_id| !active_ids.contains(&model_pricing_id))
    {
        return Err(StoreError::PricingSyncConflict);
    }

    let mut inserted_targets = HashSet::new();
    for record in &changes.insert_model_pricing {
        let target = (
            record.pricing_provider_id.clone(),
            record.pricing_model_id.clone(),
        );
        if !inserted_targets.insert(target.clone()) {
            return Err(StoreError::Conflict(format!(
                "model pricing sync contains duplicate target `{}/{}`",
                target.0, target.1
            )));
        }

        if active_by_target
            .get(&target)
            .is_some_and(|active_id| !close_ids.contains(active_id))
            || state.targets_starting_at_effective_at.contains(&target)
        {
            return Err(StoreError::PricingSyncConflict);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;

    use gateway_core::{
        ModelPricingRecord, ModelPricingSyncChanges, PricingCatalogCacheRecord,
        PricingCatalogRepository, PricingLimits, PricingModalities, PricingProvenance,
    };
    use serial_test::serial;
    use tempfile::tempdir;
    use time::OffsetDateTime;
    use url::Url;

    use super::*;
    use crate::{
        LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
        run_migrations_with_options,
    };

    fn pricing_record(model_pricing_id: Uuid) -> ModelPricingRecord {
        pricing_record_for_model(model_pricing_id, "gpt-5")
    }

    fn pricing_record_for_model(
        model_pricing_id: Uuid,
        pricing_model_id: &str,
    ) -> ModelPricingRecord {
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp");
        ModelPricingRecord {
            model_pricing_id,
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: pricing_model_id.to_string(),
            display_name: "GPT-5".to_string(),
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            input_audio_cost_per_million_tokens: None,
            output_audio_cost_per_million_tokens: None,
            output_image_usd_per_million: None,
            context_tiers: Vec::new(),
            release_date: "2025-01-01".to_string(),
            last_updated: "2025-01-01".to_string(),
            effective_start_at: now,
            effective_end_at: None,
            limits: PricingLimits {
                context: None,
                input: None,
                output: None,
            },
            modalities: PricingModalities {
                input: Vec::new(),
                output: Vec::new(),
            },
            provenance: PricingProvenance {
                source: "test".to_string(),
                etag: None,
                fetched_at: now,
            },
            created_at: now,
            updated_at: now,
        }
    }

    async fn assert_concurrent_syncs_serialize<S>(first_store: S, second_store: S)
    where
        S: PricingCatalogRepository + Clone + Send + Sync,
    {
        let model_id = "gpt-5-concurrent";
        let initial = pricing_record_for_model(Uuid::new_v4(), model_id);
        first_store
            .insert_model_pricing(&initial)
            .await
            .expect("insert concurrent test pricing");

        let effective_at = initial.provenance.fetched_at + time::Duration::seconds(1);
        let changes = |replacement_id| ModelPricingSyncChanges {
            close_model_pricing_ids: vec![initial.model_pricing_id],
            insert_model_pricing: vec![{
                let mut replacement = pricing_record_for_model(replacement_id, model_id);
                replacement.effective_start_at = effective_at;
                replacement.provenance.fetched_at = effective_at;
                replacement.created_at = effective_at;
                replacement.updated_at = effective_at;
                replacement
            }],
            ..Default::default()
        };
        let first_changes = changes(Uuid::new_v4());
        let second_changes = changes(Uuid::new_v4());
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));

        let first_barrier = barrier.clone();
        let first = async move {
            first_barrier.wait().await;
            first_store
                .apply_model_pricing_sync(&first_changes, effective_at)
                .await
        };
        let second = async move {
            barrier.wait().await;
            second_store
                .apply_model_pricing_sync(&second_changes, effective_at)
                .await
        };
        let (first_result, second_result) = tokio::join!(first, second);

        assert!(
            matches!(
                (&first_result, &second_result),
                (Ok(()), Err(StoreError::PricingSyncConflict))
                    | (Err(StoreError::PricingSyncConflict), Ok(()))
            ),
            "one concurrent sync should win and one should retry: first={first_result:?}, second={second_result:?}"
        );
    }

    async fn assert_stale_cache_write_does_not_replace_newer_snapshot<S>(
        first_store: S,
        second_store: S,
    ) where
        S: PricingCatalogRepository + Clone + Send + Sync,
    {
        let initial_fetched_at =
            OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp");
        let cache_record =
            |etag: &str, snapshot_json: &str, fetched_at| PricingCatalogCacheRecord {
                catalog_key: "concurrent-catalog".to_string(),
                source: "test".to_string(),
                etag: Some(etag.to_string()),
                fetched_at,
                snapshot_json: snapshot_json.to_string(),
            };
        assert!(
            first_store
                .compare_and_swap_pricing_catalog_cache(
                    &cache_record("initial", "initial", initial_fetched_at),
                    None,
                )
                .await
                .expect("seed cache generation")
        );
        let read_store = first_store.clone();

        let stale_cache = cache_record(
            "stale",
            "stale",
            initial_fetched_at + time::Duration::seconds(3),
        );
        let newer_cache = cache_record(
            "newer",
            "newer",
            initial_fetched_at + time::Duration::seconds(2),
        );
        assert!(
            second_store
                .compare_and_swap_pricing_catalog_cache(&newer_cache, Some(initial_fetched_at),)
                .await
                .expect("write newer cache snapshot")
        );
        assert!(
            !first_store
                .compare_and_swap_pricing_catalog_cache(&stale_cache, Some(initial_fetched_at),)
                .await
                .expect("reject stale cache snapshot")
        );

        let stored = read_store
            .get_pricing_catalog_cache("concurrent-catalog")
            .await
            .expect("load cache generation")
            .expect("cache generation");
        assert_eq!(
            stored.fetched_at,
            initial_fetched_at + time::Duration::seconds(2)
        );
        assert_eq!(stored.snapshot_json, "newer");
    }

    async fn assert_stale_sync_and_rollback_are_rejected(store: &impl PricingCatalogRepository) {
        let initial = pricing_record(Uuid::new_v4());
        store
            .insert_model_pricing(&initial)
            .await
            .expect("insert initial pricing");

        let first_at = initial.provenance.fetched_at + time::Duration::minutes(1);
        let mut first = pricing_record(Uuid::new_v4());
        first.effective_start_at = first_at;
        first.provenance.fetched_at = first_at;
        first.created_at = first_at;
        first.updated_at = first_at;
        let first_changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![initial.model_pricing_id],
            insert_model_pricing: vec![first.clone()],
            ..Default::default()
        };
        let stale_changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![initial.model_pricing_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };

        store
            .apply_model_pricing_sync(&first_changes, first_at)
            .await
            .expect("apply first pricing sync");
        let stale_error = store
            .apply_model_pricing_sync(&stale_changes, first_at)
            .await
            .expect_err("stale pricing sync should be rejected");
        assert!(matches!(stale_error, StoreError::PricingSyncConflict));

        let stale_insert = ModelPricingSyncChanges {
            insert_model_pricing: vec![pricing_record_for_model(
                Uuid::new_v4(),
                "removed-by-newer-snapshot",
            )],
            ..Default::default()
        };
        let stale_insert_error = store
            .apply_model_pricing_sync(&stale_insert, initial.provenance.fetched_at)
            .await
            .expect_err("stale insert should not resurrect a removed target");
        assert!(matches!(
            stale_insert_error,
            StoreError::PricingSyncConflict
        ));

        let rollback_changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![first.model_pricing_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };
        let rollback_error = store
            .apply_model_pricing_sync(&rollback_changes, initial.provenance.fetched_at)
            .await
            .expect_err("older pricing snapshot should be rejected");
        assert!(matches!(rollback_error, StoreError::PricingSyncConflict));

        let active = store
            .list_active_model_pricing()
            .await
            .expect("list active pricing");
        let active_for_model = active
            .iter()
            .filter(|row| row.pricing_model_id == "gpt-5")
            .collect::<Vec<_>>();
        assert_eq!(active_for_model.len(), 1);
        assert_eq!(active_for_model[0].model_pricing_id, first.model_pricing_id);
    }

    fn active_row(
        model_pricing_id: Uuid,
        pricing_model_id: &str,
        updated_at: OffsetDateTime,
    ) -> ActiveModelPricing {
        ActiveModelPricing {
            model_pricing_id,
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: pricing_model_id.to_string(),
            updated_at,
        }
    }

    fn sync_state(active_rows: Vec<ActiveModelPricing>) -> ModelPricingSyncState {
        ModelPricingSyncState {
            active_rows,
            catalog_cache_fetched_at: HashMap::new(),
            targets_starting_at_effective_at: HashSet::new(),
        }
    }

    #[test]
    fn rejects_changes_computed_from_a_replaced_active_row() {
        let original_id = Uuid::new_v4();
        let replacement_id = Uuid::new_v4();
        let changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![original_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };
        let replaced_at = pricing_record(replacement_id).updated_at;
        let state = sync_state(vec![active_row(replacement_id, "gpt-5", replaced_at)]);

        let effective_at = replaced_at + time::Duration::seconds(1);
        let error = validate_model_pricing_sync(&changes, effective_at, state)
            .expect_err("stale sync should be rejected");

        assert!(matches!(error, StoreError::PricingSyncConflict));
    }

    #[test]
    fn accepts_replacing_the_expected_active_row_from_current_cache_rows() {
        let original_id = Uuid::new_v4();
        let synced_at = pricing_record(original_id).updated_at;
        let changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![original_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            catalog_cache_fetched_at: [
                ("cached-catalog".to_string(), synced_at),
                ("vendored-catalog".to_string(), synced_at),
            ]
            .into(),
            ..Default::default()
        };
        let mut state = sync_state(vec![active_row(original_id, "gpt-5", synced_at)]);
        // The vendored source has no cache row.
        state
            .catalog_cache_fetched_at
            .insert("cached-catalog".to_string(), synced_at);

        let effective_at = synced_at + time::Duration::seconds(1);
        validate_model_pricing_sync(&changes, effective_at, state)
            .expect("current sync should be valid");
    }

    #[test]
    fn rejects_changes_computed_from_a_replaced_cache_row() {
        let fetched_at = pricing_record(Uuid::new_v4()).provenance.fetched_at;
        let changes = ModelPricingSyncChanges {
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            catalog_cache_fetched_at: [("catalog".to_string(), fetched_at)].into(),
            ..Default::default()
        };
        let mut state = sync_state(Vec::new());
        state.catalog_cache_fetched_at.insert(
            "catalog".to_string(),
            fetched_at + time::Duration::seconds(1),
        );

        let error = validate_model_pricing_sync(&changes, fetched_at, state)
            .expect_err("a sync of a replaced catalog snapshot should be rejected");

        assert!(matches!(error, StoreError::PricingSyncConflict));
    }

    #[test]
    fn rejects_a_sync_that_does_not_follow_the_expected_active_row() {
        let original_id = Uuid::new_v4();
        let synced_at = pricing_record(original_id).updated_at;
        let changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![original_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };

        // Replacing a row at its own start would duplicate that start.
        for effective_at in [synced_at - time::Duration::seconds(1), synced_at] {
            let state = sync_state(vec![active_row(original_id, "gpt-5", synced_at)]);
            let error = validate_model_pricing_sync(&changes, effective_at, state)
                .expect_err("a sync must follow the row it replaces");

            assert!(matches!(error, StoreError::PricingSyncConflict));
        }
    }

    #[test]
    fn rejects_a_stale_insert_when_another_target_is_newer() {
        let active_id = Uuid::new_v4();
        let synced_at = pricing_record(active_id).updated_at + time::Duration::seconds(1);
        let changes = ModelPricingSyncChanges {
            insert_model_pricing: vec![pricing_record_for_model(
                Uuid::new_v4(),
                "removed-by-newer-snapshot",
            )],
            ..Default::default()
        };
        let state = sync_state(vec![active_row(active_id, "current-model", synced_at)]);

        let effective_at = changes.insert_model_pricing[0].effective_start_at;
        let error = validate_model_pricing_sync(&changes, effective_at, state)
            .expect_err("stale insert must not resurrect a removed target");

        assert!(matches!(error, StoreError::PricingSyncConflict));
    }

    #[test]
    fn rejects_an_insert_whose_target_already_has_a_row_at_the_sync_time() {
        let changes = ModelPricingSyncChanges {
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };
        let mut state = sync_state(Vec::new());
        state
            .targets_starting_at_effective_at
            .insert(("openai".to_string(), "gpt-5".to_string()));

        let effective_at = changes.insert_model_pricing[0].effective_start_at;
        let error = validate_model_pricing_sync(&changes, effective_at, state)
            .expect_err("a closed row already starts at the sync time");

        assert!(matches!(error, StoreError::PricingSyncConflict));
    }

    async fn assert_fallback_lookup_prefers_exact_id(store: &impl PricingCatalogRepository) {
        let occurred_at = pricing_record(Uuid::new_v4()).effective_start_at;
        let resolve = |model_id| {
            store.resolve_model_pricing_with_fallback_at(
                "openai",
                model_id,
                "fallback-model@default",
                occurred_at,
            )
        };

        assert!(
            resolve("fallback-model")
                .await
                .expect("resolve without rows")
                .is_none()
        );

        let fallback = pricing_record_for_model(Uuid::new_v4(), "fallback-model@default");
        store
            .insert_model_pricing(&fallback)
            .await
            .expect("insert fallback pricing");
        let resolved = resolve("fallback-model")
            .await
            .expect("resolve fallback")
            .expect("fallback row");
        assert_eq!(resolved.pricing_model_id, "fallback-model@default");

        let exact = pricing_record_for_model(Uuid::new_v4(), "fallback-model");
        store
            .insert_model_pricing(&exact)
            .await
            .expect("insert exact pricing");
        let resolved = resolve("fallback-model")
            .await
            .expect("resolve exact")
            .expect("exact row");
        assert_eq!(resolved.pricing_model_id, "fallback-model");
    }

    /// Two combinations of catalog snapshots share `generation`; the newer one replaces
    /// rows of the older one without a second row at the same start.
    async fn assert_same_generation_sync_conflicts_then_converges(
        store: &impl PricingCatalogRepository,
    ) {
        let model_id = "gpt-5-same-generation";
        let catalog_key = "same-generation-catalog";
        let generation = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("timestamp");
        let older_fetched_at = generation - time::Duration::minutes(2);
        let newer_fetched_at = generation - time::Duration::minutes(1);
        let cache_record = |fetched_at| PricingCatalogCacheRecord {
            catalog_key: catalog_key.to_string(),
            source: "test".to_string(),
            etag: None,
            fetched_at,
            snapshot_json: "{}".to_string(),
        };
        let row = |fetched_at, effective_at| {
            let mut row = pricing_record_for_model(Uuid::new_v4(), model_id);
            row.provenance.fetched_at = fetched_at;
            row.effective_start_at = effective_at;
            row.created_at = effective_at;
            row.updated_at = effective_at;
            row
        };
        let changes = |close: &ModelPricingRecord,
                       insert: ModelPricingRecord,
                       fetched_at: OffsetDateTime| ModelPricingSyncChanges {
            close_model_pricing_ids: vec![close.model_pricing_id],
            insert_model_pricing: vec![insert],
            catalog_cache_fetched_at: [(catalog_key.to_string(), fetched_at)].into(),
            ..Default::default()
        };

        assert!(
            store
                .compare_and_swap_pricing_catalog_cache(&cache_record(older_fetched_at), None)
                .await
                .expect("seed older catalog snapshot")
        );
        let first = row(older_fetched_at, generation);
        store
            .apply_model_pricing_sync(
                &ModelPricingSyncChanges {
                    insert_model_pricing: vec![first.clone()],
                    catalog_cache_fetched_at: [(catalog_key.to_string(), older_fetched_at)].into(),
                    ..Default::default()
                },
                generation,
            )
            .await
            .expect("apply the older combination");
        assert!(
            store
                .compare_and_swap_pricing_catalog_cache(
                    &cache_record(newer_fetched_at),
                    Some(older_fetched_at),
                )
                .await
                .expect("write newer catalog snapshot")
        );

        let same_start_error = store
            .apply_model_pricing_sync(
                &changes(&first, row(newer_fetched_at, generation), newer_fetched_at),
                generation,
            )
            .await
            .expect_err("a second row at the same start must be a retryable conflict");
        assert!(
            matches!(same_start_error, StoreError::PricingSyncConflict),
            "{same_start_error:?}"
        );

        let replaced_at = generation + time::Duration::seconds(1);
        let stale_error = store
            .apply_model_pricing_sync(
                &changes(&first, row(older_fetched_at, replaced_at), older_fetched_at),
                replaced_at,
            )
            .await
            .expect_err("the older combination must not replace its own rows");
        assert!(matches!(stale_error, StoreError::PricingSyncConflict));

        let latest = row(newer_fetched_at, replaced_at);
        store
            .apply_model_pricing_sync(
                &changes(&first, latest.clone(), newer_fetched_at),
                replaced_at,
            )
            .await
            .expect("the newer combination replaces the older rows");
        let active = store
            .list_active_model_pricing()
            .await
            .expect("list active pricing")
            .into_iter()
            .filter(|record| record.pricing_model_id == model_id)
            .collect::<Vec<_>>();
        assert_eq!(active, vec![latest]);
    }

    #[tokio::test]
    #[serial]
    async fn libsql_rejects_stale_reconciliation_and_catalog_rollback() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let second_store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("second store");

        assert_stale_cache_write_does_not_replace_newer_snapshot(
            store.clone(),
            second_store.clone(),
        )
        .await;
        assert_concurrent_syncs_serialize(store.clone(), second_store).await;
        assert_stale_sync_and_rollback_are_rejected(&store).await;
        assert_fallback_lookup_prefers_exact_id(&store).await;
        // Last: its rows are newer than the syncs of the helpers above.
        assert_same_generation_sync_conflicts_then_converges(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_rejects_stale_reconciliation_and_catalog_rollback() {
        let Ok(base_url) = env::var("TEST_POSTGRES_URL") else {
            eprintln!("skipping postgres pricing sync test because TEST_POSTGRES_URL is not set");
            return;
        };
        let mut admin_url = Url::parse(&base_url).expect("valid postgres url");
        admin_url.set_path("/postgres");
        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(admin_url.as_str())
            .await
            .expect("admin postgres pool");
        let database_name = format!("gateway_store_test_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {database_name}"))
            .execute(&admin_pool)
            .await
            .expect("create test database");
        admin_pool.close().await;

        let mut database_url = Url::parse(&base_url).expect("valid postgres url");
        database_url.set_path(&format!("/{database_name}"));
        let options = StoreConnectionOptions::Postgres {
            url: database_url.to_string(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");
        let store = PostgresStore::connect(database_url.as_str(), 4)
            .await
            .expect("postgres store");

        assert_stale_cache_write_does_not_replace_newer_snapshot(store.clone(), store.clone())
            .await;
        assert_concurrent_syncs_serialize(store.clone(), store.clone()).await;
        assert_stale_sync_and_rollback_are_rejected(&store).await;
        assert_fallback_lookup_prefers_exact_id(&store).await;
        // Last: its rows are newer than the syncs of the helpers above.
        assert_same_generation_sync_conflicts_then_converges(&store).await;

        drop(store);
        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(admin_url.as_str())
            .await
            .expect("admin postgres pool");
        sqlx::query(&format!("DROP DATABASE {database_name}"))
            .execute(&admin_pool)
            .await
            .expect("drop test database");
        admin_pool.close().await;
    }
}
