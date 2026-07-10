use std::collections::{HashMap, HashSet};

use gateway_core::{ModelPricingSyncChanges, StoreError};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) struct ActiveModelPricing {
    pub(crate) model_pricing_id: Uuid,
    pub(crate) pricing_provider_id: String,
    pub(crate) pricing_model_id: String,
    pub(crate) provenance_fetched_at: OffsetDateTime,
}

pub(crate) fn validate_model_pricing_sync(
    changes: &ModelPricingSyncChanges,
    effective_at: OffsetDateTime,
    active_rows: impl IntoIterator<Item = ActiveModelPricing>,
) -> Result<(), StoreError> {
    let mut active_ids = HashMap::new();
    let mut active_by_target = HashMap::new();
    for row in active_rows {
        if effective_at < row.provenance_fetched_at {
            return Err(StoreError::PricingSyncConflict);
        }
        active_ids.insert(row.model_pricing_id, row.provenance_fetched_at);
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
    if expected_active_ids.into_iter().any(|model_pricing_id| {
        active_ids
            .get(&model_pricing_id)
            .is_none_or(|fetched_at| effective_at <= *fetched_at)
    }) {
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

    async fn assert_concurrent_cache_writes_allocate_distinct_generations<S>(
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
        first_store
            .upsert_pricing_catalog_cache(&cache_record("initial", "initial", initial_fetched_at))
            .await
            .expect("seed cache generation");
        let read_store = first_store.clone();

        let requested_generation = initial_fetched_at + time::Duration::seconds(1);
        let first_cache = cache_record("first", "first", requested_generation);
        let second_cache = cache_record("second", "second", requested_generation);
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));

        let first_barrier = barrier.clone();
        let first = async move {
            first_barrier.wait().await;
            first_store.upsert_pricing_catalog_cache(&first_cache).await
        };
        let second = async move {
            barrier.wait().await;
            second_store
                .upsert_pricing_catalog_cache(&second_cache)
                .await
        };
        let (first_result, second_result) = tokio::join!(first, second);
        first_result.expect("first concurrent cache write");
        second_result.expect("second concurrent cache write");

        let stored = read_store
            .get_pricing_catalog_cache("concurrent-catalog")
            .await
            .expect("load cache generation")
            .expect("cache generation");
        assert_eq!(
            stored.fetched_at,
            initial_fetched_at + time::Duration::seconds(2)
        );
        assert!(matches!(stored.snapshot_json.as_str(), "first" | "second"));
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

    #[test]
    fn rejects_changes_computed_from_a_replaced_active_row() {
        let original_id = Uuid::new_v4();
        let replacement_id = Uuid::new_v4();
        let changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![original_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };
        let active_rows = [ActiveModelPricing {
            model_pricing_id: replacement_id,
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: "gpt-5".to_string(),
            provenance_fetched_at: pricing_record(replacement_id).provenance.fetched_at,
        }];

        let effective_at = changes.insert_model_pricing[0].provenance.fetched_at;
        let error = validate_model_pricing_sync(&changes, effective_at, active_rows)
            .expect_err("stale sync should be rejected");

        assert!(matches!(error, StoreError::PricingSyncConflict));
    }

    #[test]
    fn accepts_replacing_the_expected_active_row() {
        let original_id = Uuid::new_v4();
        let changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![original_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };
        let active_rows = [ActiveModelPricing {
            model_pricing_id: original_id,
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: "gpt-5".to_string(),
            provenance_fetched_at: pricing_record(original_id).provenance.fetched_at,
        }];

        let effective_at =
            changes.insert_model_pricing[0].provenance.fetched_at + time::Duration::seconds(1);
        validate_model_pricing_sync(&changes, effective_at, active_rows)
            .expect("current sync should be valid");
    }

    #[test]
    fn rejects_an_older_snapshot_for_the_expected_active_row() {
        let original_id = Uuid::new_v4();
        let active_record = pricing_record(original_id);
        let changes = ModelPricingSyncChanges {
            close_model_pricing_ids: vec![original_id],
            insert_model_pricing: vec![pricing_record(Uuid::new_v4())],
            ..Default::default()
        };
        let active_rows = [ActiveModelPricing {
            model_pricing_id: original_id,
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: "gpt-5".to_string(),
            provenance_fetched_at: active_record.provenance.fetched_at,
        }];

        let error = validate_model_pricing_sync(
            &changes,
            active_record.provenance.fetched_at - time::Duration::seconds(1),
            active_rows,
        )
        .expect_err("older snapshot should not replace a newer active row");

        assert!(matches!(error, StoreError::PricingSyncConflict));
    }

    #[test]
    fn rejects_a_stale_insert_when_another_target_is_newer() {
        let active_id = Uuid::new_v4();
        let mut active_record = pricing_record_for_model(active_id, "current-model");
        active_record.provenance.fetched_at += time::Duration::seconds(1);
        let changes = ModelPricingSyncChanges {
            insert_model_pricing: vec![pricing_record_for_model(
                Uuid::new_v4(),
                "removed-by-newer-snapshot",
            )],
            ..Default::default()
        };
        let active_rows = [ActiveModelPricing {
            model_pricing_id: active_id,
            pricing_provider_id: active_record.pricing_provider_id,
            pricing_model_id: active_record.pricing_model_id,
            provenance_fetched_at: active_record.provenance.fetched_at,
        }];

        let effective_at = changes.insert_model_pricing[0].provenance.fetched_at;
        let error = validate_model_pricing_sync(&changes, effective_at, active_rows)
            .expect_err("stale insert must not resurrect a removed target");

        assert!(matches!(error, StoreError::PricingSyncConflict));
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

        assert_concurrent_cache_writes_allocate_distinct_generations(
            store.clone(),
            second_store.clone(),
        )
        .await;
        assert_concurrent_syncs_serialize(store.clone(), second_store).await;
        assert_stale_sync_and_rollback_are_rejected(&store).await;
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

        assert_concurrent_cache_writes_allocate_distinct_generations(store.clone(), store.clone())
            .await;
        assert_concurrent_syncs_serialize(store.clone(), store.clone()).await;
        assert_stale_sync_and_rollback_are_rejected(&store).await;

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
