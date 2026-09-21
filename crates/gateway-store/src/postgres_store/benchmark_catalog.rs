use super::*;

#[async_trait]
impl BenchmarkCatalogRepository for PostgresStore {
    async fn replace_model_benchmark_bindings(
        &self,
        source: &str,
        bindings: &[ModelBenchmarkBinding],
    ) -> Result<(), StoreError> {
        let desired = crate::benchmark_catalog::validate_bindings(source, bindings)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('oceans_llm_benchmark_bindings'))")
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;

        let existing_model_ids = sqlx::query(
            "SELECT model_id FROM model_benchmark_bindings WHERE source = $1 FOR UPDATE",
        )
        .bind(source)
        .fetch_all(&mut *tx)
        .await
        .map_err(to_query_error)?
        .into_iter()
        .map(|row| crate::shared::parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?))
        .collect::<Result<Vec<_>, StoreError>>()?;

        for model_id in existing_model_ids {
            if !desired.contains(&model_id) {
                sqlx::query(
                    "DELETE FROM model_benchmark_bindings WHERE model_id = $1 AND source = $2",
                )
                .bind(model_id.to_string())
                .bind(source)
                .execute(&mut *tx)
                .await
                .map_err(to_write_error)?;
            }
        }

        for binding in bindings {
            sqlx::query(
                r#"
                INSERT INTO model_benchmark_bindings (model_id, source, source_model_id)
                VALUES ($1, $2, $3)
                ON CONFLICT(model_id, source) DO UPDATE SET
                    source_model_id = excluded.source_model_id
                "#,
            )
            .bind(binding.model_id.to_string())
            .bind(binding.source.as_str())
            .bind(binding.source_model_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
            sqlx::query(
                r#"
                DELETE FROM model_benchmark_scores
                WHERE model_id = $1 AND source = $2 AND source_model_id <> $3
                "#,
            )
            .bind(binding.model_id.to_string())
            .bind(binding.source.as_str())
            .bind(binding.source_model_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        }

        tx.commit().await.map_err(to_write_error)
    }

    async fn list_model_benchmark_bindings(
        &self,
        source: &str,
    ) -> Result<Vec<ModelBenchmarkBinding>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT model_id, source, source_model_id
            FROM model_benchmark_bindings
            WHERE source = $1
            ORDER BY model_id ASC
            "#,
        )
        .bind(source)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode_binding).collect()
    }

    async fn list_model_benchmark_scores(&self) -> Result<Vec<ModelBenchmarkScore>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT model_id, metric_key, label, value, unit, benchmark_version,
                   source, source_model_id, source_url, fetched_at
            FROM model_benchmark_scores
            ORDER BY model_id ASC, source ASC, metric_key ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode_score).collect()
    }

    async fn get_benchmark_sync_state(
        &self,
        source: &str,
    ) -> Result<Option<BenchmarkSyncState>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT source, benchmark_version, last_successful_refresh_at, updated_at
            FROM benchmark_sync_state
            WHERE source = $1
            LIMIT 1
            "#,
        )
        .bind(source)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        row.as_ref().map(decode_sync_state).transpose()
    }

    async fn replace_model_benchmark_scores(
        &self,
        scores: &[ModelBenchmarkScore],
        state: &BenchmarkSyncState,
    ) -> Result<bool, StoreError> {
        crate::benchmark_catalog::validate_scores(scores, state)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtext('oceans_llm_benchmark_catalog_refresh'))",
        )
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        let current_fetched_at = sqlx::query_scalar::<_, i64>(
            "SELECT last_successful_refresh_at FROM benchmark_sync_state WHERE source = $1",
        )
        .bind(state.source.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?;
        if current_fetched_at
            .is_some_and(|current| current >= state.last_successful_refresh_at.unix_timestamp())
        {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(false);
        }

        sqlx::query("DELETE FROM model_benchmark_scores WHERE source = $1")
            .bind(state.source.as_str())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;

        for score in scores {
            let result = sqlx::query(
                r#"
                INSERT INTO model_benchmark_scores (
                    model_id, source, metric_key, label, value, unit, benchmark_version,
                    source_model_id, source_url, fetched_at
                )
                SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10
                WHERE EXISTS (
                    SELECT 1 FROM model_benchmark_bindings
                    WHERE model_id = $1 AND source = $2 AND source_model_id = $8
                )
                "#,
            )
            .bind(score.model_id.to_string())
            .bind(score.source.as_str())
            .bind(score.metric_key.as_str())
            .bind(score.label.as_str())
            .bind(score.value)
            .bind(score.unit.as_str())
            .bind(score.benchmark_version.as_str())
            .bind(score.source_model_id.as_str())
            .bind(score.source_url.as_str())
            .bind(score.fetched_at.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
            if result.rows_affected() != 1 {
                return Err(StoreError::Conflict(format!(
                    "benchmark score binding changed for model `{}`",
                    score.model_id
                )));
            }
        }

        sqlx::query(
            r#"
            INSERT INTO benchmark_sync_state (
                source, benchmark_version, last_successful_refresh_at, updated_at
            ) VALUES ($1, $2, $3, $4)
            ON CONFLICT(source) DO UPDATE SET
                benchmark_version = excluded.benchmark_version,
                last_successful_refresh_at = excluded.last_successful_refresh_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(state.source.as_str())
        .bind(state.benchmark_version.as_str())
        .bind(state.last_successful_refresh_at.unix_timestamp())
        .bind(state.updated_at.unix_timestamp())
        .execute(&mut *tx)
        .await
        .map_err(to_write_error)?;

        tx.commit().await.map_err(to_write_error)?;
        Ok(true)
    }
}

fn decode_binding(row: &PgRow) -> Result<ModelBenchmarkBinding, StoreError> {
    Ok(ModelBenchmarkBinding {
        model_id: crate::shared::parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        source: row.try_get(1).map_err(to_query_error)?,
        source_model_id: row.try_get(2).map_err(to_query_error)?,
    })
}

fn decode_score(row: &PgRow) -> Result<ModelBenchmarkScore, StoreError> {
    Ok(ModelBenchmarkScore {
        model_id: crate::shared::parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        metric_key: row.try_get(1).map_err(to_query_error)?,
        label: row.try_get(2).map_err(to_query_error)?,
        value: row.try_get(3).map_err(to_query_error)?,
        unit: row.try_get(4).map_err(to_query_error)?,
        benchmark_version: row.try_get(5).map_err(to_query_error)?,
        source: row.try_get(6).map_err(to_query_error)?,
        source_model_id: row.try_get(7).map_err(to_query_error)?,
        source_url: row.try_get(8).map_err(to_query_error)?,
        fetched_at: crate::shared::unix_to_datetime(row.try_get(9).map_err(to_query_error)?)?,
    })
}

fn decode_sync_state(row: &PgRow) -> Result<BenchmarkSyncState, StoreError> {
    Ok(BenchmarkSyncState {
        source: row.try_get(0).map_err(to_query_error)?,
        benchmark_version: row.try_get(1).map_err(to_query_error)?,
        last_successful_refresh_at: crate::shared::unix_to_datetime(
            row.try_get(2).map_err(to_query_error)?,
        )?,
        updated_at: crate::shared::unix_to_datetime(row.try_get(3).map_err(to_query_error)?)?,
    })
}
