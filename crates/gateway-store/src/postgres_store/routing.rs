use std::collections::HashSet;

use async_trait::async_trait;
use gateway_core::{
    ResponseRouteOrigin, RouteBindingReceipt, RouteFailureRecord, RouteSelection,
    RouteSelectionMode, RouteSelectionRequest, RoutingRepository, StoreError,
};
use sqlx::{Executor, Postgres, Row, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{PostgresStore, support::to_query_error};
use crate::{
    routing::{
        RESPONSE_ORIGIN_RETENTION_SECONDS, StoredRouteBinding, binding_expiry, choose_route,
        cooldown_keys, new_selection, reuse_binding, routing_pool_key,
    },
    shared::parse_uuid,
};

async fn load_reusable_binding<'e>(
    executor: impl Executor<'e, Database = Postgres>,
    request: &RouteSelectionRequest,
) -> Result<Option<RouteSelection>, StoreError> {
    let Some(affinity_key) = &request.affinity_key else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT route_id, route_fingerprint, binding_token, expires_at
         FROM model_route_bindings WHERE model_id = $1 AND affinity_key = $2",
    )
    .bind(request.model_id.to_string())
    .bind(affinity_key)
    .fetch_optional(executor)
    .await
    .map_err(to_query_error)?;
    row.as_ref()
        .map(decode_binding)
        .transpose()
        .map(|binding| binding.and_then(|binding| reuse_binding(request, &binding)))
}

fn decode_binding(row: &PgRow) -> Result<StoredRouteBinding, StoreError> {
    Ok(StoredRouteBinding {
        route_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        fingerprint: row.try_get(1).map_err(to_query_error)?,
        token: parse_uuid(&row.try_get::<String, _>(2).map_err(to_query_error)?)?,
        expires_at: row.try_get(3).map_err(to_query_error)?,
    })
}

fn decode_response_origin(row: &PgRow) -> Result<ResponseRouteOrigin, StoreError> {
    Ok(ResponseRouteOrigin {
        model_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        route_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        fingerprint: row.try_get(2).map_err(to_query_error)?,
    })
}

async fn lock_cooldown(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    key: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended('oceans:model_route_cooldown:' || $1, 0)
        )",
    )
    .bind(key)
    .execute(&mut **tx)
    .await
    .map_err(to_query_error)?;
    Ok(())
}

async fn lock_affinity(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    model_id: Uuid,
    affinity_key: Option<&str>,
) -> Result<(), StoreError> {
    if let Some(affinity_key) = affinity_key {
        sqlx::query(
            "SELECT pg_advisory_xact_lock(
                hashtextextended('oceans:model_routing:' || $1 || ':' || $2, 0)
            )",
        )
        .bind(model_id.to_string())
        .bind(affinity_key)
        .execute(&mut **tx)
        .await
        .map_err(to_query_error)?;
    }
    Ok(())
}

async fn exclude_cooling_routes(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    request: &mut RouteSelectionRequest,
) -> Result<(), StoreError> {
    let keys = cooldown_keys(request);
    if keys.is_empty() {
        return Ok(());
    }
    // Every selector and failure writer locks cooldown identities before affinity.
    // Sorting gives overlapping candidate pools the same advisory lock order.
    for key in &keys {
        sqlx::query(
            "SELECT pg_advisory_xact_lock_shared(
                hashtextextended('oceans:model_route_cooldown:' || $1, 0)
            )",
        )
        .bind(key)
        .execute(&mut **tx)
        .await
        .map_err(to_query_error)?;
    }
    let active = sqlx::query_scalar::<_, String>(
        "SELECT cooldown_key FROM model_route_cooldowns
         WHERE cooldown_key = ANY($1) AND expires_at > $2",
    )
    .bind(&keys)
    .bind(request.now.unix_timestamp())
    .fetch_all(&mut **tx)
    .await
    .map_err(to_query_error)?
    .into_iter()
    .collect::<HashSet<_>>();
    request.candidates.retain(|candidate| {
        !candidate
            .cooldown_key
            .as_ref()
            .is_some_and(|key| active.contains(key))
    });
    Ok(())
}

async fn load_cursor(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    pool_key: &str,
) -> Result<i64, StoreError> {
    sqlx::query(
        "INSERT INTO model_routing_cursors (pool_key, next_index) VALUES ($1, 0)
         ON CONFLICT (pool_key) DO NOTHING",
    )
    .bind(pool_key)
    .execute(&mut **tx)
    .await
    .map_err(to_query_error)?;
    sqlx::query_scalar(
        "SELECT next_index FROM model_routing_cursors WHERE pool_key = $1 FOR UPDATE",
    )
    .bind(pool_key)
    .fetch_one(&mut **tx)
    .await
    .map_err(to_query_error)
}

async fn cleanup_cooldowns(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    // Cleanup is the final lock-taking operation. Do not hold unrelated cooldown
    // rows while waiting for an affinity lock needed by a failure writer.
    sqlx::query(
        "DELETE FROM model_route_cooldowns WHERE cooldown_key IN (
            SELECT cooldown_key FROM model_route_cooldowns
            WHERE expires_at <= $1 ORDER BY expires_at LIMIT 64
            FOR UPDATE SKIP LOCKED
        )",
    )
    .bind(now.unix_timestamp())
    .execute(&mut **tx)
    .await
    .map_err(to_query_error)?;
    Ok(())
}

#[async_trait]
impl RoutingRepository for PostgresStore {
    async fn select_route(
        &self,
        request: &RouteSelectionRequest,
    ) -> Result<Option<RouteSelection>, StoreError> {
        binding_expiry(request.now, request.idle_timeout_seconds)?;
        let (candidate, _) = choose_route(request, 0)?;
        let checks_cooldowns = !cooldown_keys(request).is_empty();
        if !checks_cooldowns
            && request.affinity_key.is_none()
            && request.mode == RouteSelectionMode::First
        {
            return Ok(Some(new_selection(request, candidate)));
        }
        if !checks_cooldowns
            && let Some(selection) = load_reusable_binding(&self.pool, request).await?
        {
            return Ok(Some(selection));
        }

        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let mut request = request.clone();
        exclude_cooling_routes(&mut tx, &mut request).await?;
        if request.candidates.is_empty() {
            cleanup_cooldowns(&mut tx, request.now).await?;
            tx.commit().await.map_err(to_query_error)?;
            return Ok(None);
        }
        // Different eligible pools must still agree on the same session's binding.
        lock_affinity(&mut tx, request.model_id, request.affinity_key.as_deref()).await?;
        // Recheck after locking in case another request created the session's binding.
        if let Some(selection) = load_reusable_binding(&mut *tx, &request).await? {
            if checks_cooldowns {
                cleanup_cooldowns(&mut tx, request.now).await?;
            }
            tx.commit().await.map_err(to_query_error)?;
            return Ok(Some(selection));
        }
        let (pool_key, cursor) = if request.mode == RouteSelectionMode::RoundRobin {
            let pool_key = routing_pool_key(&request)?.to_string();
            let cursor = load_cursor(&mut tx, &pool_key).await?;
            (Some(pool_key), cursor)
        } else {
            (None, 0)
        };
        let (candidate, next_cursor) = choose_route(&request, cursor)?;
        let selection = new_selection(&request, candidate);
        if let Some(pool_key) = pool_key {
            sqlx::query("UPDATE model_routing_cursors SET next_index = $1 WHERE pool_key = $2")
                .bind(next_cursor)
                .bind(&pool_key)
                .execute(&mut *tx)
                .await
                .map_err(to_query_error)?;
        }
        if let Some(receipt) = &selection.binding {
            persist_binding(&mut *tx, receipt, &candidate.fingerprint, request.now).await?;
        }

        // Take the current binding lock before cleanup, and skip rows held by other requests.
        sqlx::query(
            "DELETE FROM model_route_bindings
             WHERE (model_id, affinity_key) IN (
                 SELECT model_id, affinity_key FROM model_route_bindings
                 WHERE expires_at <= $1 ORDER BY expires_at LIMIT 64
                 FOR UPDATE SKIP LOCKED
             )",
        )
        .bind(request.now.unix_timestamp())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;
        if checks_cooldowns {
            cleanup_cooldowns(&mut tx, request.now).await?;
        }
        tx.commit().await.map_err(to_query_error)?;
        Ok(Some(selection))
    }

    async fn record_route_failure(&self, failure: &RouteFailureRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        if let Some(key) = &failure.cooldown_key {
            lock_cooldown(&mut tx, key).await?;
        }
        if let Some(binding) = &failure.binding {
            lock_affinity(&mut tx, binding.model_id, Some(&binding.affinity_key)).await?;
        }
        if let Some(key) = &failure.cooldown_key {
            sqlx::query(
                "INSERT INTO model_route_cooldowns (cooldown_key, expires_at)
                 VALUES ($1, $2) ON CONFLICT(cooldown_key) DO UPDATE SET
                    expires_at = GREATEST(model_route_cooldowns.expires_at, EXCLUDED.expires_at)",
            )
            .bind(key)
            .bind(failure.cooldown_until.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
        }
        if let Some(binding) = &failure.binding {
            sqlx::query(
                "DELETE FROM model_route_bindings WHERE model_id = $1 AND affinity_key = $2
                 AND binding_token = $3 AND route_id = $4",
            )
            .bind(binding.model_id.to_string())
            .bind(&binding.affinity_key)
            .bind(binding.token.to_string())
            .bind(binding.route_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
        }
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }

    async fn refresh_route_binding(
        &self,
        receipt: &RouteBindingReceipt,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE model_route_bindings SET expires_at = GREATEST(expires_at, $1)
             WHERE model_id = $2 AND affinity_key = $3 AND binding_token = $4
                 AND route_id = $5",
        )
        .bind(binding_expiry(now, receipt.idle_timeout_seconds)?)
        .bind(receipt.model_id.to_string())
        .bind(&receipt.affinity_key)
        .bind(receipt.token.to_string())
        .bind(receipt.route_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }

    async fn get_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ResponseRouteOrigin>, StoreError> {
        let row = sqlx::query(
            "SELECT model_id, route_id, route_fingerprint FROM response_route_origins
             WHERE owner_key = $1 AND response_id_hash = $2 AND expires_at > $3",
        )
        .bind(owner_key)
        .bind(response_id_hash)
        .bind(now.unix_timestamp())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        row.as_ref().map(decode_response_origin).transpose()
    }

    async fn record_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        origin: &ResponseRouteOrigin,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "INSERT INTO response_route_origins (
                 owner_key, response_id_hash, model_id, route_id, route_fingerprint, expires_at
             ) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (owner_key, response_id_hash) DO UPDATE
             SET expires_at = GREATEST(response_route_origins.expires_at, EXCLUDED.expires_at)
             WHERE response_route_origins.model_id = EXCLUDED.model_id
                 AND response_route_origins.route_id = EXCLUDED.route_id
                 AND response_route_origins.route_fingerprint = EXCLUDED.route_fingerprint",
        )
        .bind(owner_key)
        .bind(response_id_hash)
        .bind(origin.model_id.to_string())
        .bind(origin.route_id.to_string())
        .bind(&origin.fingerprint)
        .bind(binding_expiry(now, RESPONSE_ORIGIN_RETENTION_SECONDS)?)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::Conflict(
                "response ID is already bound to another route".to_string(),
            ));
        }
        sqlx::query(
            "DELETE FROM response_route_origins
             WHERE (owner_key, response_id_hash) IN (
                 SELECT owner_key, response_id_hash FROM response_route_origins
                 WHERE expires_at <= $1 ORDER BY expires_at LIMIT 64
                 FOR UPDATE SKIP LOCKED
             )",
        )
        .bind(now.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}

async fn persist_binding<'e>(
    executor: impl Executor<'e, Database = Postgres>,
    receipt: &RouteBindingReceipt,
    fingerprint: &str,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO model_route_bindings (
             model_id, affinity_key, route_id, route_fingerprint, binding_token, expires_at
         ) VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (model_id, affinity_key) DO UPDATE SET
             route_id = EXCLUDED.route_id,
             route_fingerprint = EXCLUDED.route_fingerprint,
             binding_token = EXCLUDED.binding_token,
             expires_at = EXCLUDED.expires_at",
    )
    .bind(receipt.model_id.to_string())
    .bind(&receipt.affinity_key)
    .bind(receipt.route_id.to_string())
    .bind(fingerprint)
    .bind(receipt.token.to_string())
    .bind(binding_expiry(now, receipt.idle_timeout_seconds)?)
    .execute(executor)
    .await
    .map_err(to_query_error)?;
    Ok(())
}
