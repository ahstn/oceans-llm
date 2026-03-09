use std::collections::HashMap;

use gateway_core::{
    SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY, SeedApiKey, SeedModel, SeedProvider, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::LibsqlStore;

impl LibsqlStore {
    pub async fn seed_from_inputs(
        &self,
        providers: &[SeedProvider],
        models: &[SeedModel],
        api_keys: &[SeedApiKey],
    ) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();

        self.connection()
            .execute(
                r#"
                INSERT OR IGNORE INTO teams (
                    team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                ) VALUES (?1, ?2, 'System Legacy', 'active', 'all', ?3, ?3)
                "#,
                libsql::params![SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY, now],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        for provider in providers {
            let config_json = serde_json::to_string(&provider.config)
                .map_err(|error| StoreError::Serialization(error.to_string()))?;
            let secrets_json = provider
                .secrets
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| StoreError::Serialization(error.to_string()))?;

            self.connection()
                .execute(
                    r#"
                    INSERT INTO providers (
                        provider_key, provider_type, config_json, secrets_json, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                    ON CONFLICT(provider_key) DO UPDATE SET
                        provider_type = excluded.provider_type,
                        config_json = excluded.config_json,
                        secrets_json = excluded.secrets_json,
                        updated_at = excluded.updated_at
                    "#,
                    libsql::params![
                        provider.provider_key.as_str(),
                        provider.provider_type.as_str(),
                        config_json,
                        secrets_json,
                        now
                    ],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;
        }

        let mut model_ids = HashMap::new();
        for model in models {
            let model_id = model_uuid(&model.model_key);
            model_ids.insert(model.model_key.clone(), model_id);
            let tags_json = serde_json::to_string(&model.tags)
                .map_err(|error| StoreError::Serialization(error.to_string()))?;

            self.connection()
                .execute(
                    r#"
                    INSERT INTO gateway_models (
                        id, model_key, description, tags_json, rank, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                    ON CONFLICT(model_key) DO UPDATE SET
                        description = excluded.description,
                        tags_json = excluded.tags_json,
                        rank = excluded.rank,
                        updated_at = excluded.updated_at
                    "#,
                    libsql::params![
                        model_id.to_string(),
                        model.model_key.as_str(),
                        model.description.clone(),
                        tags_json,
                        model.rank,
                        now
                    ],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;

            self.connection()
                .execute(
                    "DELETE FROM model_routes WHERE model_id = ?1",
                    [model_id.to_string()],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;

            for (route_index, route) in model.routes.iter().enumerate() {
                let route_id = route_uuid(
                    &model.model_key,
                    &route.provider_key,
                    &route.upstream_model,
                    route.priority,
                    route_index,
                );

                let extra_headers_json = serde_json::to_string(&route.extra_headers)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?;
                let extra_body_json = serde_json::to_string(&route.extra_body)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?;

                self.connection()
                    .execute(
                        r#"
                        INSERT INTO model_routes (
                            id, model_id, provider_key, upstream_model, priority, weight, enabled,
                            extra_headers_json, extra_body_json, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
                        ON CONFLICT(id) DO UPDATE SET
                            weight = excluded.weight,
                            enabled = excluded.enabled,
                            extra_headers_json = excluded.extra_headers_json,
                            extra_body_json = excluded.extra_body_json,
                            updated_at = excluded.updated_at
                        "#,
                        libsql::params![
                            route_id.to_string(),
                            model_id.to_string(),
                            route.provider_key.as_str(),
                            route.upstream_model.as_str(),
                            route.priority,
                            route.weight,
                            if route.enabled { 1_i64 } else { 0_i64 },
                            extra_headers_json,
                            extra_body_json,
                            now
                        ],
                    )
                    .await
                    .map_err(|error| StoreError::Query(error.to_string()))?;
            }
        }

        for api_key in api_keys {
            let key_id = api_key_uuid(&api_key.public_id);

            self.connection()
                .execute(
                    r#"
                    INSERT INTO api_keys (
                        id, public_id, secret_hash, name, status,
                        owner_kind, owner_user_id, owner_team_id, created_at
                    ) VALUES (?1, ?2, ?3, ?4, 'active', 'team', NULL, ?5, ?6)
                    ON CONFLICT(public_id) DO UPDATE SET
                        secret_hash = excluded.secret_hash,
                        name = excluded.name,
                        owner_kind = excluded.owner_kind,
                        owner_user_id = excluded.owner_user_id,
                        owner_team_id = excluded.owner_team_id
                    "#,
                    libsql::params![
                        key_id.to_string(),
                        api_key.public_id.as_str(),
                        api_key.secret_hash.as_str(),
                        api_key.name.as_str(),
                        SYSTEM_LEGACY_TEAM_ID,
                        now
                    ],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;

            self.connection()
                .execute(
                    "DELETE FROM api_key_model_grants WHERE api_key_id = ?1",
                    [key_id.to_string()],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;

            for model_key in &api_key.allowed_models {
                let model_id = model_ids.get(model_key).ok_or_else(|| {
                    StoreError::NotFound(format!(
                        "seed api key `{}` references unknown model `{model_key}`",
                        api_key.public_id
                    ))
                })?;

                self.connection()
                    .execute(
                        r#"
                        INSERT INTO api_key_model_grants (api_key_id, model_id)
                        VALUES (?1, ?2)
                        ON CONFLICT(api_key_id, model_id) DO NOTHING
                        "#,
                        libsql::params![key_id.to_string(), model_id.to_string()],
                    )
                    .await
                    .map_err(|error| StoreError::Query(error.to_string()))?;
            }
        }

        Ok(())
    }
}

pub(crate) fn model_uuid(model_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("model:{model_key}").as_bytes(),
    )
}

pub(crate) fn route_uuid(
    model_key: &str,
    provider_key: &str,
    upstream_model: &str,
    priority: i32,
    route_index: usize,
) -> Uuid {
    let key = format!("route:{model_key}:{provider_key}:{upstream_model}:{priority}:{route_index}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes())
}

pub(crate) fn api_key_uuid(public_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("api_key:{public_id}").as_bytes(),
    )
}
