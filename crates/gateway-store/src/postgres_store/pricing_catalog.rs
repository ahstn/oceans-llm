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
}
