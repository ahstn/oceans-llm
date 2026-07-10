use super::*;

#[async_trait]
impl PricingCatalogRepository for PostgresStore {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT catalog_key, source, etag, fetched_at, snapshot_json
            FROM pricing_catalog_cache
            WHERE catalog_key = $1
            LIMIT 1
            "#,
        )
        .bind(catalog_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref()
            .map(decode_pricing_catalog_cache_record)
            .transpose()
    }

    async fn upsert_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO pricing_catalog_cache (
                catalog_key, source, etag, fetched_at, snapshot_json
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT(catalog_key) DO UPDATE SET
                source = excluded.source,
                etag = excluded.etag,
                fetched_at = excluded.fetched_at,
                snapshot_json = excluded.snapshot_json
            "#,
        )
        .bind(cache.catalog_key.as_str())
        .bind(cache.source.as_str())
        .bind(cache.etag.as_deref())
        .bind(cache.fetched_at.unix_timestamp())
        .bind(cache.snapshot_json.as_str())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }

    async fn touch_pricing_catalog_cache_fetched_at(
        &self,
        catalog_key: &str,
        fetched_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE pricing_catalog_cache
            SET fetched_at = $1
            WHERE catalog_key = $2
            "#,
        )
        .bind(fetched_at.unix_timestamp())
        .bind(catalog_key)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }

    async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                model_pricing_id, pricing_provider_id, pricing_model_id, display_name,
                input_cost_per_million_tokens_10000,
                output_cost_per_million_tokens_10000,
                cache_read_cost_per_million_tokens_10000,
                cache_write_cost_per_million_tokens_10000,
                input_audio_cost_per_million_tokens_10000,
                output_audio_cost_per_million_tokens_10000,
                release_date, last_updated, effective_start_at, effective_end_at,
                limits_json, modalities_json, provenance_source, provenance_etag,
                provenance_fetched_at, created_at, updated_at
            FROM model_pricing
            WHERE effective_end_at IS NULL
            ORDER BY pricing_provider_id ASC, pricing_model_id ASC, effective_start_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_model_pricing_record).collect()
    }

    async fn insert_model_pricing(&self, record: &ModelPricingRecord) -> Result<(), StoreError> {
        let limits_json = crate::shared::serialize_json(&record.limits)?;
        let modalities_json = crate::shared::serialize_json(&record.modalities)?;

        sqlx::query(
            r#"
            INSERT INTO model_pricing (
                model_pricing_id, pricing_provider_id, pricing_model_id, display_name,
                input_cost_per_million_tokens_10000,
                output_cost_per_million_tokens_10000,
                cache_read_cost_per_million_tokens_10000,
                cache_write_cost_per_million_tokens_10000,
                input_audio_cost_per_million_tokens_10000,
                output_audio_cost_per_million_tokens_10000,
                release_date, last_updated, effective_start_at, effective_end_at,
                limits_json, modalities_json, provenance_source, provenance_etag,
                provenance_fetched_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                $15, $16, $17, $18, $19, $20, $21
            )
            "#,
        )
        .bind(record.model_pricing_id.to_string())
        .bind(record.pricing_provider_id.as_str())
        .bind(record.pricing_model_id.as_str())
        .bind(record.display_name.as_str())
        .bind(
            record
                .input_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(
            record
                .output_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(
            record
                .cache_read_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(
            record
                .cache_write_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(
            record
                .input_audio_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(
            record
                .output_audio_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(record.release_date.as_str())
        .bind(record.last_updated.as_str())
        .bind(record.effective_start_at.unix_timestamp())
        .bind(record.effective_end_at.map(OffsetDateTime::unix_timestamp))
        .bind(limits_json)
        .bind(modalities_json)
        .bind(record.provenance.source.as_str())
        .bind(record.provenance.etag.as_deref())
        .bind(record.provenance.fetched_at.unix_timestamp())
        .bind(record.created_at.unix_timestamp())
        .bind(record.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn close_model_pricing(
        &self,
        model_pricing_id: Uuid,
        effective_end_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE model_pricing
            SET effective_end_at = $2,
                updated_at = $3
            WHERE model_pricing_id = $1
            "#,
        )
        .bind(model_pricing_id.to_string())
        .bind(effective_end_at.unix_timestamp())
        .bind(updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn apply_model_pricing_sync(
        &self,
        changes: &ModelPricingSyncChanges,
        effective_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;

        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtext('oceans_llm_model_pricing_reconciliation'))",
        )
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        let active_rows = sqlx::query(
            r#"
            SELECT model_pricing_id, pricing_provider_id, pricing_model_id,
                   provenance_fetched_at
            FROM model_pricing
            WHERE effective_end_at IS NULL
            FOR UPDATE
            "#,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(to_query_error)?
        .into_iter()
        .map(|row| {
            Ok(crate::pricing_sync::ActiveModelPricing {
                model_pricing_id: crate::shared::parse_uuid(
                    &row.try_get::<String, _>(0).map_err(to_query_error)?,
                )?,
                pricing_provider_id: row.try_get(1).map_err(to_query_error)?,
                pricing_model_id: row.try_get(2).map_err(to_query_error)?,
                provenance_fetched_at: crate::shared::unix_to_datetime(
                    row.try_get(3).map_err(to_query_error)?,
                )?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
        crate::pricing_sync::validate_model_pricing_sync(changes, effective_at, active_rows)?;

        for model_pricing_id in &changes.close_model_pricing_ids {
            sqlx::query(
                r#"
                UPDATE model_pricing
                SET effective_end_at = $2,
                    updated_at = $3
                WHERE model_pricing_id = $1
                "#,
            )
            .bind(model_pricing_id.to_string())
            .bind(effective_at.unix_timestamp())
            .bind(effective_at.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        }

        for update in &changes.update_provenance {
            sqlx::query(
                r#"
                UPDATE model_pricing
                SET provenance_source = $2,
                    provenance_etag = $3,
                    provenance_fetched_at = $4,
                    updated_at = $5
                WHERE model_pricing_id = $1
                "#,
            )
            .bind(update.model_pricing_id.to_string())
            .bind(update.provenance.source.as_str())
            .bind(update.provenance.etag.as_deref())
            .bind(update.provenance.fetched_at.unix_timestamp())
            .bind(update.updated_at.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        }

        for record in &changes.insert_model_pricing {
            let limits_json = crate::shared::serialize_json(&record.limits)?;
            let modalities_json = crate::shared::serialize_json(&record.modalities)?;

            sqlx::query(
                r#"
                INSERT INTO model_pricing (
                    model_pricing_id, pricing_provider_id, pricing_model_id, display_name,
                    input_cost_per_million_tokens_10000,
                    output_cost_per_million_tokens_10000,
                    cache_read_cost_per_million_tokens_10000,
                    cache_write_cost_per_million_tokens_10000,
                    input_audio_cost_per_million_tokens_10000,
                    output_audio_cost_per_million_tokens_10000,
                    release_date, last_updated, effective_start_at, effective_end_at,
                    limits_json, modalities_json, provenance_source, provenance_etag,
                    provenance_fetched_at, created_at, updated_at
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21
                )
                "#,
            )
            .bind(record.model_pricing_id.to_string())
            .bind(record.pricing_provider_id.as_str())
            .bind(record.pricing_model_id.as_str())
            .bind(record.display_name.as_str())
            .bind(
                record
                    .input_cost_per_million_tokens
                    .map(Money4::as_scaled_i64),
            )
            .bind(
                record
                    .output_cost_per_million_tokens
                    .map(Money4::as_scaled_i64),
            )
            .bind(
                record
                    .cache_read_cost_per_million_tokens
                    .map(Money4::as_scaled_i64),
            )
            .bind(
                record
                    .cache_write_cost_per_million_tokens
                    .map(Money4::as_scaled_i64),
            )
            .bind(
                record
                    .input_audio_cost_per_million_tokens
                    .map(Money4::as_scaled_i64),
            )
            .bind(
                record
                    .output_audio_cost_per_million_tokens
                    .map(Money4::as_scaled_i64),
            )
            .bind(record.release_date.as_str())
            .bind(record.last_updated.as_str())
            .bind(record.effective_start_at.unix_timestamp())
            .bind(record.effective_end_at.map(OffsetDateTime::unix_timestamp))
            .bind(limits_json)
            .bind(modalities_json)
            .bind(record.provenance.source.as_str())
            .bind(record.provenance.etag.as_deref())
            .bind(record.provenance.fetched_at.unix_timestamp())
            .bind(record.created_at.unix_timestamp())
            .bind(record.updated_at.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        }

        tx.commit().await.map_err(to_write_error)?;
        Ok(())
    }

    async fn resolve_model_pricing_at(
        &self,
        pricing_provider_id: &str,
        pricing_model_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<Option<ModelPricingRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT
                model_pricing_id, pricing_provider_id, pricing_model_id, display_name,
                input_cost_per_million_tokens_10000,
                output_cost_per_million_tokens_10000,
                cache_read_cost_per_million_tokens_10000,
                cache_write_cost_per_million_tokens_10000,
                input_audio_cost_per_million_tokens_10000,
                output_audio_cost_per_million_tokens_10000,
                release_date, last_updated, effective_start_at, effective_end_at,
                limits_json, modalities_json, provenance_source, provenance_etag,
                provenance_fetched_at, created_at, updated_at
            FROM model_pricing
            WHERE pricing_provider_id = $1
              AND pricing_model_id = $2
              AND effective_start_at <= $3
              AND (effective_end_at IS NULL OR effective_end_at > $3)
            ORDER BY effective_start_at DESC
            LIMIT 1
            "#,
        )
        .bind(pricing_provider_id)
        .bind(pricing_model_id)
        .bind(occurred_at.unix_timestamp())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_model_pricing_record).transpose()
    }
}
