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

        let fetched_at: i64 = row.get(3).map_err(to_query_error)?;
        Ok(Some(PricingCatalogCacheRecord {
            catalog_key: row.get(0).map_err(to_query_error)?,
            source: row.get(1).map_err(to_query_error)?,
            etag: row.get(2).map_err(to_query_error)?,
            fetched_at: crate::shared::unix_to_datetime(fetched_at)?,
            snapshot_json: row.get(4).map_err(to_query_error)?,
        }))
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
}
