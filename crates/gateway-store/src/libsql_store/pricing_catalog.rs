use super::*;

#[async_trait]
impl PricingCatalogRepository for LibsqlStore {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT catalog_key, source, etag, fetched_at, snapshot_json
                FROM pricing_catalog_cache
                WHERE catalog_key = ?1
                LIMIT 1
                "#,
                [catalog_key],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_pricing_catalog_cache_record(&row).map(Some)
    }

    async fn upsert_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO pricing_catalog_cache (
                    catalog_key, source, etag, fetched_at, snapshot_json
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(catalog_key) DO UPDATE SET
                    source = excluded.source,
                    etag = excluded.etag,
                    fetched_at = excluded.fetched_at,
                    snapshot_json = excluded.snapshot_json
                "#,
                libsql::params![
                    cache.catalog_key.as_str(),
                    cache.source.as_str(),
                    cache.etag.as_deref(),
                    cache.fetched_at.unix_timestamp(),
                    cache.snapshot_json.as_str(),
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        Ok(())
    }

    async fn touch_pricing_catalog_cache_fetched_at(
        &self,
        catalog_key: &str,
        fetched_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE pricing_catalog_cache
                SET fetched_at = ?1
                WHERE catalog_key = ?2
                "#,
                libsql::params![fetched_at.unix_timestamp(), catalog_key],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        Ok(())
    }

    async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
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
                (),
            )
            .await
            .map_err(to_query_error)?;

        let mut records = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            records.push(decode_model_pricing_record(&row)?);
        }

        Ok(records)
    }

    async fn insert_model_pricing(&self, record: &ModelPricingRecord) -> Result<(), StoreError> {
        let limits_json = crate::shared::serialize_json(&record.limits)?;
        let modalities_json = crate::shared::serialize_json(&record.modalities)?;

        self.connection
            .execute(
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
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18, ?19, ?20, ?21
                )
                "#,
                libsql::params![
                    record.model_pricing_id.to_string(),
                    record.pricing_provider_id.as_str(),
                    record.pricing_model_id.as_str(),
                    record.display_name.as_str(),
                    record
                        .input_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    record
                        .output_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    record
                        .cache_read_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    record
                        .cache_write_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    record
                        .input_audio_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    record
                        .output_audio_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    record.release_date.as_str(),
                    record.last_updated.as_str(),
                    record.effective_start_at.unix_timestamp(),
                    record.effective_end_at.map(OffsetDateTime::unix_timestamp),
                    limits_json,
                    modalities_json,
                    record.provenance.source.as_str(),
                    record.provenance.etag.as_deref(),
                    record.provenance.fetched_at.unix_timestamp(),
                    record.created_at.unix_timestamp(),
                    record.updated_at.unix_timestamp()
                ],
            )
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
        self.connection
            .execute(
                r#"
                UPDATE model_pricing
                SET effective_end_at = ?2,
                    updated_at = ?3
                WHERE model_pricing_id = ?1
                "#,
                libsql::params![
                    model_pricing_id.to_string(),
                    effective_end_at.unix_timestamp(),
                    updated_at.unix_timestamp()
                ],
            )
            .await
            .map_err(to_write_error)?;

        Ok(())
    }

    async fn resolve_model_pricing_at(
        &self,
        pricing_provider_id: &str,
        pricing_model_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<Option<ModelPricingRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
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
                WHERE pricing_provider_id = ?1
                  AND pricing_model_id = ?2
                  AND effective_start_at <= ?3
                  AND (effective_end_at IS NULL OR effective_end_at > ?3)
                ORDER BY effective_start_at DESC
                LIMIT 1
                "#,
                libsql::params![
                    pricing_provider_id,
                    pricing_model_id,
                    occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_model_pricing_record(&row).map(Some)
    }
}
