use super::*;
use crate::shared::{serialize_json, serialize_optional_json};
use crate::seed::{reconcile_seed_teams, reconcile_seed_users};

impl PostgresStore {
    pub async fn seed_update_identity_user_profile(
        &self,
        user_id: Uuid,
        name: &str,
        email: &str,
        email_normalized: &str,
        request_logging_enabled: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE users
            SET name = $1,
                email = $2,
                email_normalized = $3,
                request_logging_enabled = $4,
                updated_at = $5
            WHERE user_id = $6
            "#,
        )
        .bind(name)
        .bind(email)
        .bind(email_normalized)
        .bind(if request_logging_enabled { 1_i64 } else { 0_i64 })
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn seed_from_inputs(
        &self,
        providers: &[gateway_core::SeedProvider],
        models: &[gateway_core::SeedModel],
        api_keys: &[gateway_core::SeedApiKey],
        teams: &[gateway_core::SeedTeam],
        users: &[gateway_core::SeedUser],
    ) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc();
        let now_unix = now.unix_timestamp();

        sqlx::query(
            r#"
            INSERT INTO teams (
                team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES ($1, $2, 'System Legacy', 'active', 'all', $3, $3)
            ON CONFLICT(team_id) DO NOTHING
            "#,
        )
        .bind(SYSTEM_LEGACY_TEAM_ID)
        .bind(SYSTEM_LEGACY_TEAM_KEY)
        .bind(now_unix)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

        for provider in providers {
            let config_json = serialize_json(&provider.config)?;
            let secrets_json = serialize_optional_json(provider.secrets.as_ref())?;

            sqlx::query(
                r#"
                INSERT INTO providers (
                    provider_key, provider_type, config_json, secrets_json, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $5)
                ON CONFLICT(provider_key) DO UPDATE SET
                    provider_type = excluded.provider_type,
                    config_json = excluded.config_json,
                    secrets_json = excluded.secrets_json,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(provider.provider_key.as_str())
            .bind(provider.provider_type.as_str())
            .bind(config_json)
            .bind(secrets_json)
            .bind(now_unix)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        }

        let mut model_ids = std::collections::HashMap::new();
        for model in models {
            model_ids.insert(model.model_key.clone(), model_uuid(&model.model_key));
        }

        // Insert model rows first with a null alias target so config order does not
        // matter for self-referential alias foreign keys.
        for model in models {
            let model_id = *model_ids
                .get(&model.model_key)
                .expect("model ids populated before insert");
            let tags_json = serialize_json(&model.tags)?;

            sqlx::query(
                r#"
                INSERT INTO gateway_models (
                    id, model_key, alias_target_model_id, description, tags_json, rank, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
                ON CONFLICT(model_key) DO UPDATE SET
                    alias_target_model_id = excluded.alias_target_model_id,
                    description = excluded.description,
                    tags_json = excluded.tags_json,
                    rank = excluded.rank,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(model_id.to_string())
            .bind(model.model_key.as_str())
            .bind(Option::<String>::None)
            .bind(model.description.clone())
            .bind(tags_json)
            .bind(model.rank)
            .bind(now_unix)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;

            sqlx::query("DELETE FROM model_routes WHERE model_id = $1")
                .bind(model_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;

            for (route_index, route) in model.routes.iter().enumerate() {
                let route_id = route_uuid(
                    &model.model_key,
                    &route.provider_key,
                    &route.upstream_model,
                    route.priority,
                    route_index,
                );
                let extra_headers_json = serialize_json(&route.extra_headers)?;
                let extra_body_json = serialize_json(&route.extra_body)?;
                let capabilities_json = serialize_json(&route.capabilities)?;

                sqlx::query(
                    r#"
                    INSERT INTO model_routes (
                        id, model_id, provider_key, upstream_model, priority, weight, enabled,
                        extra_headers_json, extra_body_json, capabilities_json, created_at, updated_at
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)
                    ON CONFLICT(id) DO UPDATE SET
                        weight = excluded.weight,
                        enabled = excluded.enabled,
                        extra_headers_json = excluded.extra_headers_json,
                        extra_body_json = excluded.extra_body_json,
                        capabilities_json = excluded.capabilities_json,
                        updated_at = excluded.updated_at
                    "#,
                )
                .bind(route_id.to_string())
                .bind(model_id.to_string())
                .bind(route.provider_key.as_str())
                .bind(route.upstream_model.as_str())
                .bind(route.priority)
                .bind(route.weight)
                .bind(if route.enabled { 1_i64 } else { 0_i64 })
                .bind(extra_headers_json)
                .bind(extra_body_json)
                .bind(capabilities_json)
                .bind(now_unix)
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;
            }
        }

        for model in models {
            let model_id = *model_ids
                .get(&model.model_key)
                .expect("model ids populated before alias update");
            let alias_target_model_id = model
                .alias_target_model_key
                .as_ref()
                .map(|model_key| {
                    model_ids.get(model_key).copied().ok_or_else(|| {
                        StoreError::NotFound(format!(
                            "seed model `{}` aliases unknown model `{model_key}`",
                            model.model_key
                        ))
                    })
                })
                .transpose()?;

            sqlx::query(
                r#"
                UPDATE gateway_models
                SET alias_target_model_id = $1, updated_at = $2
                WHERE id = $3
                "#,
            )
            .bind(alias_target_model_id.map(|value| value.to_string()))
            .bind(now_unix)
            .bind(model_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        }

        for api_key in api_keys {
            let key_id = api_key_uuid(&api_key.public_id);

            sqlx::query(
                r#"
                INSERT INTO api_keys (
                    id, public_id, secret_hash, name, status,
                    owner_kind, owner_user_id, owner_team_id, created_at
                ) VALUES ($1, $2, $3, $4, 'active', 'team', NULL, $5, $6)
                ON CONFLICT(public_id) DO UPDATE SET
                    secret_hash = excluded.secret_hash,
                    name = excluded.name,
                    owner_kind = excluded.owner_kind,
                    owner_user_id = excluded.owner_user_id,
                    owner_team_id = excluded.owner_team_id
                "#,
            )
            .bind(key_id.to_string())
            .bind(api_key.public_id.as_str())
            .bind(api_key.secret_hash.as_str())
            .bind(api_key.name.as_str())
            .bind(SYSTEM_LEGACY_TEAM_ID)
            .bind(now_unix)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;

            sqlx::query("DELETE FROM api_key_model_grants WHERE api_key_id = $1")
                .bind(key_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;

            for model_key in &api_key.allowed_models {
                let model_id = model_ids.get(model_key).ok_or_else(|| {
                    StoreError::NotFound(format!(
                        "seed api key `{}` references unknown model `{model_key}`",
                        api_key.public_id
                    ))
                })?;

                sqlx::query(
                    r#"
                    INSERT INTO api_key_model_grants (api_key_id, model_id)
                    VALUES ($1, $2)
                    ON CONFLICT(api_key_id, model_id) DO NOTHING
                    "#,
                )
                .bind(key_id.to_string())
                .bind(model_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;
            }
        }

        let seeded_teams = reconcile_seed_teams(self, teams, now).await?;
        reconcile_seed_users(self, &seeded_teams, users, now).await?;

        Ok(())
    }
}
