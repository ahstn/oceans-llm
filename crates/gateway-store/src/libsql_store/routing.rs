use async_trait::async_trait;
use gateway_core::{
    ResponseRouteOrigin, RouteBindingReceipt, RouteSelection, RouteSelectionMode,
    RouteSelectionRequest, RoutingCandidate, RoutingRepository, StoreError,
};
use time::OffsetDateTime;

use super::{LibsqlStore, support::to_query_error};
use crate::routing::{
    RESPONSE_ORIGIN_RETENTION_SECONDS, StoredRouteBinding, binding_expiry, choose_route,
    new_selection, reuse_binding, routing_pool_key,
};
use crate::shared::parse_uuid;

impl LibsqlStore {
    async fn run_routing_operation<T, F, Fut>(&self, operation: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(libsql::Connection) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, StoreError>> + Send,
    {
        // Local libsql async methods execute SQLite synchronously. Acquire a
        // bounded worker permit before spawning so lock waits cannot stall Tokio.
        let permit = self
            .routing_workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        let database = self.database.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            // Transactions own their connection; a clone of the ordinary
            // connection would share a transaction with unrelated requests.
            let connection = database.connect().map_err(to_query_error)?;
            connection
                .busy_timeout(std::time::Duration::from_secs(3))
                .map_err(to_query_error)?;
            runtime.block_on(operation(connection))
        })
        .await
        .map_err(|error| StoreError::Unavailable(error.to_string()))?
    }
}

async fn load_binding(
    connection: &libsql::Connection,
    request: &RouteSelectionRequest,
) -> Result<Option<RouteSelection>, StoreError> {
    let Some(affinity_key) = &request.affinity_key else {
        return Ok(None);
    };
    let mut rows = connection
        .query(
            "SELECT route_id, route_fingerprint, binding_token, expires_at
             FROM model_route_bindings WHERE model_id = ?1 AND affinity_key = ?2",
            libsql::params![request.model_id.to_string(), affinity_key.as_str()],
        )
        .await
        .map_err(to_query_error)?;
    let Some(row) = rows.next().await.map_err(to_query_error)? else {
        return Ok(None);
    };
    let binding = StoredRouteBinding {
        route_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        fingerprint: row.get(1).map_err(to_query_error)?,
        token: parse_uuid(&row.get::<String>(2).map_err(to_query_error)?)?,
        expires_at: row.get(3).map_err(to_query_error)?,
    };
    Ok(reuse_binding(request, &binding))
}

async fn load_cursor(connection: &libsql::Connection, pool_key: &str) -> Result<i64, StoreError> {
    connection
        .execute(
            "INSERT INTO model_routing_cursors (pool_key, next_index) VALUES (?1, 0)
             ON CONFLICT(pool_key) DO NOTHING",
            [pool_key],
        )
        .await
        .map_err(to_query_error)?;
    let mut rows = connection
        .query(
            "SELECT next_index FROM model_routing_cursors WHERE pool_key = ?1",
            [pool_key],
        )
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::Unexpected("routing cursor is missing".to_string()))?
        .get(0)
        .map_err(to_query_error)
}

async fn save_binding(
    connection: &libsql::Connection,
    selection: &RouteSelection,
    candidate: &RoutingCandidate,
    expires_at: i64,
) -> Result<(), StoreError> {
    let Some(receipt) = &selection.binding else {
        return Ok(());
    };
    connection
        .execute(
            "INSERT INTO model_route_bindings
                (model_id, affinity_key, route_id, route_fingerprint, binding_token, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(model_id, affinity_key) DO UPDATE SET
                route_id = excluded.route_id,
                route_fingerprint = excluded.route_fingerprint,
                binding_token = excluded.binding_token,
                expires_at = excluded.expires_at",
            libsql::params![
                receipt.model_id.to_string(),
                receipt.affinity_key.as_str(),
                receipt.route_id.to_string(),
                candidate.fingerprint.as_str(),
                receipt.token.to_string(),
                expires_at
            ],
        )
        .await
        .map_err(to_query_error)?;
    Ok(())
}

#[async_trait]
impl RoutingRepository for LibsqlStore {
    async fn select_route(
        &self,
        request: &RouteSelectionRequest,
    ) -> Result<RouteSelection, StoreError> {
        let expires_at = binding_expiry(request.now, request.idle_timeout_seconds)?;
        let (candidate, _) = choose_route(request, 0)?;
        if request.affinity_key.is_none() && request.mode == RouteSelectionMode::First {
            return Ok(new_selection(request, candidate));
        }
        let request = request.clone();
        self.run_routing_operation(move |connection| async move {
            if let Some(selection) = load_binding(&connection, &request).await? {
                return Ok(selection);
            }

            let transaction = connection
                .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
                .await
                .map_err(to_query_error)?;
            // Recheck after acquiring the writer lock: another replica may have
            // assigned this session while this request waited for the transaction.
            if let Some(selection) = load_binding(&transaction, &request).await? {
                transaction.commit().await.map_err(to_query_error)?;
                return Ok(selection);
            }
            let pool_key = routing_pool_key(&request)?.to_string();
            let cursor = load_cursor(&transaction, &pool_key).await?;
            let (candidate, next_index) = choose_route(&request, cursor)?;
            let selection = new_selection(&request, candidate);
            save_binding(&transaction, &selection, candidate, expires_at).await?;
            if request.mode == RouteSelectionMode::RoundRobin {
                transaction
                    .execute(
                        "UPDATE model_routing_cursors SET next_index = ?1 WHERE pool_key = ?2",
                        libsql::params![next_index, pool_key.as_str()],
                    )
                    .await
                    .map_err(to_query_error)?;
            }
            transaction
                .execute(
                    "DELETE FROM model_route_bindings WHERE (model_id, affinity_key) IN (
                        SELECT model_id, affinity_key FROM model_route_bindings
                        WHERE expires_at <= ?1 ORDER BY expires_at LIMIT 64
                    )",
                    [request.now.unix_timestamp()],
                )
                .await
                .map_err(to_query_error)?;
            transaction.commit().await.map_err(to_query_error)?;
            Ok(selection)
        })
        .await
    }

    async fn refresh_route_binding(
        &self,
        receipt: &RouteBindingReceipt,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let expires_at = binding_expiry(now, receipt.idle_timeout_seconds)?;
        let receipt = receipt.clone();
        self.run_routing_operation(move |connection| async move {
            connection
                .execute(
                    "UPDATE model_route_bindings SET expires_at = MAX(expires_at, ?1)
                     WHERE model_id = ?2 AND affinity_key = ?3
                       AND binding_token = ?4 AND route_id = ?5",
                    libsql::params![
                        expires_at,
                        receipt.model_id.to_string(),
                        receipt.affinity_key.as_str(),
                        receipt.token.to_string(),
                        receipt.route_id.to_string()
                    ],
                )
                .await
                .map_err(to_query_error)?;
            Ok(())
        })
        .await
    }

    async fn get_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ResponseRouteOrigin>, StoreError> {
        let owner_key = owner_key.to_string();
        let response_id_hash = response_id_hash.to_string();
        self.run_routing_operation(move |connection| async move {
            let mut rows = connection
                .query(
                    "SELECT model_id, route_id, route_fingerprint FROM response_route_origins
                     WHERE owner_key = ?1 AND response_id_hash = ?2 AND expires_at > ?3",
                    libsql::params![owner_key, response_id_hash, now.unix_timestamp()],
                )
                .await
                .map_err(to_query_error)?;
            rows.next()
                .await
                .map_err(to_query_error)?
                .map(|row| {
                    Ok(ResponseRouteOrigin {
                        model_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
                        route_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
                        fingerprint: row.get(2).map_err(to_query_error)?,
                    })
                })
                .transpose()
        })
        .await
    }

    async fn record_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        origin: &ResponseRouteOrigin,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let expires_at = binding_expiry(now, RESPONSE_ORIGIN_RETENTION_SECONDS)?;
        let owner_key = owner_key.to_string();
        let response_id_hash = response_id_hash.to_string();
        let origin = origin.clone();
        self.run_routing_operation(move |connection| async move {
            let changed = connection
                .execute(
                    "INSERT INTO response_route_origins
                        (owner_key, response_id_hash, model_id, route_id, route_fingerprint, expires_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(owner_key, response_id_hash) DO UPDATE SET
                        expires_at = MAX(response_route_origins.expires_at, excluded.expires_at)
                     WHERE response_route_origins.model_id = excluded.model_id
                       AND response_route_origins.route_id = excluded.route_id
                       AND response_route_origins.route_fingerprint = excluded.route_fingerprint",
                    libsql::params![
                        owner_key,
                        response_id_hash,
                        origin.model_id.to_string(),
                        origin.route_id.to_string(),
                        origin.fingerprint.as_str(),
                        expires_at
                    ],
                )
                .await
                .map_err(to_query_error)?;
            if changed == 0 {
                return Err(StoreError::Conflict(
                    "response route origin cannot be changed".to_string(),
                ));
            }
            connection
                .execute(
                    "DELETE FROM response_route_origins WHERE (owner_key, response_id_hash) IN (
                        SELECT owner_key, response_id_hash FROM response_route_origins
                        WHERE expires_at <= ?1 ORDER BY expires_at LIMIT 64
                    )",
                    [now.unix_timestamp()],
                )
                .await
                .map_err(to_query_error)?;
            Ok(())
        })
        .await
    }
}
