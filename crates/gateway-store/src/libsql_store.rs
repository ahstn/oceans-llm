use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use gateway_core::{
    ApiKeyRecord, ApiKeyRepository, GatewayModel, ModelRepository, ModelRoute, ProviderConnection,
    ProviderRepository, StoreError, StoreHealth,
};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct LibsqlStore {
    connection: Arc<libsql::Connection>,
}

impl LibsqlStore {
    pub async fn new_local(path: &str) -> anyhow::Result<Self> {
        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .with_context(|| format!("failed building local libsql database at `{path}`"))?;
        let connection = db.connect().context("failed opening libsql connection")?;

        Ok(Self {
            connection: Arc::new(connection),
        })
    }

    pub(crate) fn connection(&self) -> &libsql::Connection {
        &self.connection
    }
}

#[async_trait]
impl StoreHealth for LibsqlStore {
    async fn ping(&self) -> Result<(), StoreError> {
        let mut rows = self
            .connection
            .query("SELECT 1", ())
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        let _ = rows
            .next()
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ApiKeyRepository for LibsqlStore {
    async fn get_api_key_by_public_id(
        &self,
        public_id: &str,
    ) -> Result<Option<ApiKeyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, public_id, secret_hash, name, status, created_at, last_used_at, revoked_at
                FROM api_keys
                WHERE public_id = ?1
                LIMIT 1
                "#,
                [public_id],
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

        decode_api_key(&row).map(Some)
    }

    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        self.connection
            .execute(
                "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
                libsql::params![now, api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ModelRepository for LibsqlStore {
    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, model_key, description, tags_json, rank
                FROM gateway_models
                WHERE model_key = ?1
                LIMIT 1
                "#,
                [model_key],
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

        decode_gateway_model(&row).map(Some)
    }

    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT gm.id, gm.model_key, gm.description, gm.tags_json, gm.rank
                FROM gateway_models gm
                INNER JOIN api_key_model_grants grants ON grants.model_id = gm.id
                WHERE grants.api_key_id = ?1
                ORDER BY gm.rank ASC, gm.model_key ASC
                "#,
                [api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut models = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            models.push(decode_gateway_model(&row)?);
        }

        Ok(models)
    }

    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled,
                       extra_headers_json, extra_body_json
                FROM model_routes
                WHERE model_id = ?1
                ORDER BY priority ASC
                "#,
                [model_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut routes = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            routes.push(decode_model_route(&row)?);
        }

        Ok(routes)
    }
}

#[async_trait]
impl ProviderRepository for LibsqlStore {
    async fn get_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<ProviderConnection>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT provider_key, provider_type, config_json, secrets_json
                FROM providers
                WHERE provider_key = ?1
                LIMIT 1
                "#,
                [provider_key],
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

        decode_provider_connection(&row).map(Some)
    }
}

fn decode_api_key(row: &libsql::Row) -> Result<ApiKeyRecord, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let created_at: i64 = row.get(5).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.get(6).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(7).map_err(to_query_error)?;

    Ok(ApiKeyRecord {
        id: Uuid::parse_str(&id).map_err(|error| StoreError::Serialization(error.to_string()))?,
        public_id: row.get(1).map_err(to_query_error)?,
        secret_hash: row.get(2).map_err(to_query_error)?,
        name: row.get(3).map_err(to_query_error)?,
        status: row.get(4).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_gateway_model(row: &libsql::Row) -> Result<GatewayModel, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let tags_json: String = row.get(3).map_err(to_query_error)?;

    Ok(GatewayModel {
        id: Uuid::parse_str(&id).map_err(|error| StoreError::Serialization(error.to_string()))?,
        model_key: row.get(1).map_err(to_query_error)?,
        description: row.get(2).map_err(to_query_error)?,
        tags: serde_json::from_str(&tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        rank: row.get(4).map_err(to_query_error)?,
    })
}

fn decode_model_route(row: &libsql::Row) -> Result<ModelRoute, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let model_id: String = row.get(1).map_err(to_query_error)?;
    let enabled: i64 = row.get(6).map_err(to_query_error)?;
    let extra_headers_json: String = row.get(7).map_err(to_query_error)?;
    let extra_body_json: String = row.get(8).map_err(to_query_error)?;

    Ok(ModelRoute {
        id: Uuid::parse_str(&id).map_err(|error| StoreError::Serialization(error.to_string()))?,
        model_id: Uuid::parse_str(&model_id)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        provider_key: row.get(2).map_err(to_query_error)?,
        upstream_model: row.get(3).map_err(to_query_error)?,
        priority: row.get(4).map_err(to_query_error)?,
        weight: row.get(5).map_err(to_query_error)?,
        enabled: enabled == 1,
        extra_headers: json_object_from_str(&extra_headers_json)?,
        extra_body: json_object_from_str(&extra_body_json)?,
    })
}

fn decode_provider_connection(row: &libsql::Row) -> Result<ProviderConnection, StoreError> {
    let config_json: String = row.get(2).map_err(to_query_error)?;
    let secrets_json: Option<String> = row.get(3).map_err(to_query_error)?;

    Ok(ProviderConnection {
        provider_key: row.get(0).map_err(to_query_error)?,
        provider_type: row.get(1).map_err(to_query_error)?,
        config: serde_json::from_str(&config_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        secrets: secrets_json
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| StoreError::Serialization(error.to_string()))
            })
            .transpose()?,
    })
}

fn json_object_from_str(value: &str) -> Result<Map<String, Value>, StoreError> {
    serde_json::from_str(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn unix_to_datetime(ts: i64) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::from_unix_timestamp(ts)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn to_query_error(error: libsql::Error) -> StoreError {
    StoreError::Query(error.to_string())
}
