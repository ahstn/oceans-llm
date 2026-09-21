use super::*;

#[async_trait]
impl BenchmarkCatalogRepository for LibsqlStore {
    async fn replace_model_benchmark_bindings(
        &self,
        source: &str,
        bindings: &[ModelBenchmarkBinding],
    ) -> Result<(), StoreError> {
        let desired = crate::benchmark_catalog::validate_bindings(source, bindings)?;
        let tx = self
            .connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_query_error)?;

        let mut rows = tx
            .query(
                "SELECT model_id FROM model_benchmark_bindings WHERE source = ?1",
                [source],
            )
            .await
            .map_err(to_query_error)?;
        let mut existing_model_ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            existing_model_ids.push(crate::shared::parse_uuid(
                &row.get::<String>(0).map_err(to_query_error)?,
            )?);
        }
        drop(rows);

        for model_id in existing_model_ids {
            if !desired.contains(&model_id) {
                tx.execute(
                    "DELETE FROM model_benchmark_bindings WHERE model_id = ?1 AND source = ?2",
                    libsql::params![model_id.to_string(), source],
                )
                .await
                .map_err(to_write_error)?;
            }
        }

        for binding in bindings {
            tx.execute(
                r#"
                INSERT INTO model_benchmark_bindings (model_id, source, source_model_id)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(model_id, source) DO UPDATE SET
                    source_model_id = excluded.source_model_id
                "#,
                libsql::params![
                    binding.model_id.to_string(),
                    binding.source.as_str(),
                    binding.source_model_id.as_str()
                ],
            )
            .await
            .map_err(to_write_error)?;
            tx.execute(
                r#"
                DELETE FROM model_benchmark_scores
                WHERE model_id = ?1 AND source = ?2 AND source_model_id <> ?3
                "#,
                libsql::params![
                    binding.model_id.to_string(),
                    binding.source.as_str(),
                    binding.source_model_id.as_str()
                ],
            )
            .await
            .map_err(to_write_error)?;
        }

        tx.commit().await.map_err(to_write_error)
    }

    async fn list_model_benchmark_bindings(
        &self,
        source: &str,
    ) -> Result<Vec<ModelBenchmarkBinding>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT model_id, source, source_model_id
                FROM model_benchmark_bindings
                WHERE source = ?1
                ORDER BY model_id ASC
                "#,
                [source],
            )
            .await
            .map_err(to_query_error)?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            bindings.push(ModelBenchmarkBinding {
                model_id: crate::shared::parse_uuid(
                    &row.get::<String>(0).map_err(to_query_error)?,
                )?,
                source: row.get(1).map_err(to_query_error)?,
                source_model_id: row.get(2).map_err(to_query_error)?,
            });
        }
        Ok(bindings)
    }

    async fn list_model_benchmark_scores(&self) -> Result<Vec<ModelBenchmarkScore>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT model_id, metric_key, label, value, unit, benchmark_version,
                       source, source_model_id, source_url, fetched_at
                FROM model_benchmark_scores
                ORDER BY model_id ASC, source ASC, metric_key ASC
                "#,
                (),
            )
            .await
            .map_err(to_query_error)?;
        let mut scores = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            scores.push(decode_score(&row)?);
        }
        Ok(scores)
    }

    async fn get_benchmark_sync_state(
        &self,
        source: &str,
    ) -> Result<Option<BenchmarkSyncState>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT source, benchmark_version, last_successful_refresh_at, updated_at
                FROM benchmark_sync_state
                WHERE source = ?1
                LIMIT 1
                "#,
                [source],
            )
            .await
            .map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };
        decode_sync_state(&row).map(Some)
    }

    async fn replace_model_benchmark_scores(
        &self,
        scores: &[ModelBenchmarkScore],
        state: &BenchmarkSyncState,
    ) -> Result<bool, StoreError> {
        crate::benchmark_catalog::validate_scores(scores, state)?;
        let tx = self
            .connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_query_error)?;

        let mut rows = tx
            .query(
                "SELECT last_successful_refresh_at FROM benchmark_sync_state WHERE source = ?1",
                [state.source.as_str()],
            )
            .await
            .map_err(to_query_error)?;
        let current_fetched_at = rows
            .next()
            .await
            .map_err(to_query_error)?
            .map(|row| row.get::<i64>(0).map_err(to_query_error))
            .transpose()?;
        drop(rows);
        if current_fetched_at
            .is_some_and(|current| current >= state.last_successful_refresh_at.unix_timestamp())
        {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(false);
        }

        tx.execute(
            "DELETE FROM model_benchmark_scores WHERE source = ?1",
            [state.source.as_str()],
        )
        .await
        .map_err(to_write_error)?;

        for score in scores {
            let inserted = tx
                .execute(
                    r#"
                    INSERT INTO model_benchmark_scores (
                        model_id, source, metric_key, label, value, unit, benchmark_version,
                        source_model_id, source_url, fetched_at
                    )
                    SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
                    WHERE EXISTS (
                        SELECT 1 FROM model_benchmark_bindings
                        WHERE model_id = ?1 AND source = ?2 AND source_model_id = ?8
                    )
                    "#,
                    libsql::params![
                        score.model_id.to_string(),
                        score.source.as_str(),
                        score.metric_key.as_str(),
                        score.label.as_str(),
                        score.value,
                        score.unit.as_str(),
                        score.benchmark_version.as_str(),
                        score.source_model_id.as_str(),
                        score.source_url.as_str(),
                        score.fetched_at.unix_timestamp()
                    ],
                )
                .await
                .map_err(to_write_error)?;
            if inserted != 1 {
                return Err(StoreError::Conflict(format!(
                    "benchmark score binding changed for model `{}`",
                    score.model_id
                )));
            }
        }

        tx.execute(
            r#"
            INSERT INTO benchmark_sync_state (
                source, benchmark_version, last_successful_refresh_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(source) DO UPDATE SET
                benchmark_version = excluded.benchmark_version,
                last_successful_refresh_at = excluded.last_successful_refresh_at,
                updated_at = excluded.updated_at
            "#,
            libsql::params![
                state.source.as_str(),
                state.benchmark_version.as_str(),
                state.last_successful_refresh_at.unix_timestamp(),
                state.updated_at.unix_timestamp()
            ],
        )
        .await
        .map_err(to_write_error)?;

        tx.commit().await.map_err(to_write_error)?;
        Ok(true)
    }
}

fn decode_score(row: &libsql::Row) -> Result<ModelBenchmarkScore, StoreError> {
    Ok(ModelBenchmarkScore {
        model_id: crate::shared::parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        metric_key: row.get(1).map_err(to_query_error)?,
        label: row.get(2).map_err(to_query_error)?,
        value: row.get(3).map_err(to_query_error)?,
        unit: row.get(4).map_err(to_query_error)?,
        benchmark_version: row.get(5).map_err(to_query_error)?,
        source: row.get(6).map_err(to_query_error)?,
        source_model_id: row.get(7).map_err(to_query_error)?,
        source_url: row.get(8).map_err(to_query_error)?,
        fetched_at: crate::shared::unix_to_datetime(row.get(9).map_err(to_query_error)?)?,
    })
}

fn decode_sync_state(row: &libsql::Row) -> Result<BenchmarkSyncState, StoreError> {
    Ok(BenchmarkSyncState {
        source: row.get(0).map_err(to_query_error)?,
        benchmark_version: row.get(1).map_err(to_query_error)?,
        last_successful_refresh_at: crate::shared::unix_to_datetime(
            row.get(2).map_err(to_query_error)?,
        )?,
        updated_at: crate::shared::unix_to_datetime(row.get(3).map_err(to_query_error)?)?,
    })
}
