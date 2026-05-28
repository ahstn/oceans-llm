use super::*;
use crate::seed::{
    prevalidate_seed_users, reconcile_seed_teams, reconcile_seed_users, service_account_uuid,
    validate_seed_api_key_team_references,
};
use crate::shared::{serialize_json, serialize_optional_json};

impl LibsqlStore {
    pub async fn seed_update_identity_user_profile(
        &self,
        user_id: Uuid,
        name: &str,
        email: &str,
        email_normalized: &str,
        request_logging_enabled: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE users
                SET name = ?1,
                    email = ?2,
                    email_normalized = ?3,
                    request_logging_enabled = ?4,
                    updated_at = ?5
                WHERE user_id = ?6
                "#,
                libsql::params![
                    name,
                    email,
                    email_normalized,
                    if request_logging_enabled {
                        1_i64
                    } else {
                        0_i64
                    },
                    updated_at.unix_timestamp(),
                    user_id.to_string()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn seed_from_inputs(
        &self,
        providers: &[gateway_core::SeedProvider],
        models: &[gateway_core::SeedModel],
        api_keys: &[gateway_core::SeedApiKey],
        oidc_providers: &[gateway_core::SeedOidcProvider],
        oauth_providers: &[gateway_core::SeedOauthProvider],
        teams: &[gateway_core::SeedTeam],
        users: &[gateway_core::SeedUser],
    ) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc();
        let now_unix = now.unix_timestamp();

        for provider in providers {
            let config_json = serialize_json(&provider.config)?;
            let secrets_json = serialize_optional_json(provider.secrets.as_ref())?;

            self.connection
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
                        now_unix
                    ],
                )
                .await
                .map_err(to_query_error)?;
        }

        for provider in oidc_providers {
            let scopes_json = serialize_json(&provider.scopes)?;
            let oidc_provider_id = crate::seed::oidc_provider_uuid(&provider.provider_key);
            self.connection
                .execute(
                    r#"
                    INSERT INTO oidc_providers (
                        oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                        scopes_json, enabled, label, client_secret_ref, jit_enabled,
                        jit_global_role, jit_team_key, jit_team_role,
                        jit_request_logging_enabled, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
                    ON CONFLICT(provider_key) DO UPDATE SET
                        provider_type = excluded.provider_type,
                        issuer_url = excluded.issuer_url,
                        client_id = excluded.client_id,
                        scopes_json = excluded.scopes_json,
                        enabled = excluded.enabled,
                        label = excluded.label,
                        client_secret_ref = excluded.client_secret_ref,
                        jit_enabled = excluded.jit_enabled,
                        jit_global_role = excluded.jit_global_role,
                        jit_team_key = excluded.jit_team_key,
                        jit_team_role = excluded.jit_team_role,
                        jit_request_logging_enabled = excluded.jit_request_logging_enabled,
                        updated_at = excluded.updated_at
                    "#,
                    libsql::params![
                        oidc_provider_id,
                        provider.provider_key.as_str(),
                        provider.provider_type.as_str(),
                        provider.issuer_url.as_str(),
                        provider.client_id.as_str(),
                        scopes_json,
                        if provider.enabled { 1_i64 } else { 0_i64 },
                        provider.label.as_str(),
                        provider.client_secret_ref.as_str(),
                        if provider.jit.enabled { 1_i64 } else { 0_i64 },
                        provider.jit.global_role.as_str(),
                        provider
                            .jit
                            .membership
                            .as_ref()
                            .map(|membership| membership.team_key.clone()),
                        provider
                            .jit
                            .membership
                            .as_ref()
                            .map(|membership| membership.role.as_str().to_string()),
                        if provider.jit.request_logging_enabled {
                            1_i64
                        } else {
                            0_i64
                        },
                        now_unix,
                    ],
                )
                .await
                .map_err(to_query_error)?;
        }

        for provider in oauth_providers {
            let scopes_json = serialize_json(&provider.scopes)?;
            let oauth_provider_id = crate::seed::oauth_provider_uuid(&provider.provider_key);
            self.connection
                .execute(
                    r#"
                    INSERT INTO oauth_providers (
                        oauth_provider_id, provider_key, provider_type, client_id,
                        scopes_json, enabled, label, client_secret_ref, jit_enabled,
                        jit_global_role, jit_team_key, jit_team_role,
                        jit_request_logging_enabled, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
                    ON CONFLICT(provider_key) DO UPDATE SET
                        provider_type = excluded.provider_type,
                        client_id = excluded.client_id,
                        scopes_json = excluded.scopes_json,
                        enabled = excluded.enabled,
                        label = excluded.label,
                        client_secret_ref = excluded.client_secret_ref,
                        jit_enabled = excluded.jit_enabled,
                        jit_global_role = excluded.jit_global_role,
                        jit_team_key = excluded.jit_team_key,
                        jit_team_role = excluded.jit_team_role,
                        jit_request_logging_enabled = excluded.jit_request_logging_enabled,
                        updated_at = excluded.updated_at
                    "#,
                    libsql::params![
                        oauth_provider_id,
                        provider.provider_key.as_str(),
                        provider.provider_type.as_str(),
                        provider.client_id.as_str(),
                        scopes_json,
                        if provider.enabled { 1_i64 } else { 0_i64 },
                        provider.label.as_str(),
                        provider.client_secret_ref.as_str(),
                        if provider.jit.enabled { 1_i64 } else { 0_i64 },
                        provider.jit.global_role.as_str(),
                        provider
                            .jit
                            .membership
                            .as_ref()
                            .map(|membership| membership.team_key.clone()),
                        provider
                            .jit
                            .membership
                            .as_ref()
                            .map(|membership| membership.role.as_str().to_string()),
                        if provider.jit.request_logging_enabled {
                            1_i64
                        } else {
                            0_i64
                        },
                        now_unix,
                    ],
                )
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

            self.connection
                .execute(
                    r#"
                    INSERT INTO gateway_models (
                        id, model_key, alias_target_model_id, description, tags_json, rank, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                    ON CONFLICT(model_key) DO UPDATE SET
                        alias_target_model_id = excluded.alias_target_model_id,
                        description = excluded.description,
                        tags_json = excluded.tags_json,
                        rank = excluded.rank,
                        updated_at = excluded.updated_at
                    "#,
                    libsql::params![
                        model_id.to_string(),
                        model.model_key.as_str(),
                        Option::<String>::None,
                        model.description.clone(),
                        tags_json,
                        model.rank,
                        now_unix
                    ],
                )
                .await
                .map_err(to_query_error)?;

            self.connection
                .execute(
                    "DELETE FROM model_routes WHERE model_id = ?1",
                    [model_id.to_string()],
                )
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
                let compatibility_json = serialize_json(&route.compatibility)?;

                self.connection
                    .execute(
                        r#"
                        INSERT INTO model_routes (
                            id, model_id, provider_key, upstream_model, priority, weight, enabled,
                            extra_headers_json, extra_body_json, capabilities_json, compatibility_json,
                            created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
                        ON CONFLICT(id) DO UPDATE SET
                            weight = excluded.weight,
                            enabled = excluded.enabled,
                            extra_headers_json = excluded.extra_headers_json,
                            extra_body_json = excluded.extra_body_json,
                            capabilities_json = excluded.capabilities_json,
                            compatibility_json = excluded.compatibility_json,
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
                            capabilities_json,
                            compatibility_json,
                            now_unix
                        ],
                    )
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

            self.connection
                .execute(
                    r#"
                    UPDATE gateway_models
                    SET alias_target_model_id = ?1, updated_at = ?2
                    WHERE id = ?3
                    "#,
                    libsql::params![
                        alias_target_model_id.map(|value| value.to_string()),
                        now_unix,
                        model_id.to_string()
                    ],
                )
                .await
                .map_err(to_query_error)?;
        }

        validate_seed_api_key_team_references(teams, api_keys)?;
        prevalidate_seed_users(self, users).await?;
        let seeded_teams = reconcile_seed_teams(self, teams, now).await?;

        for api_key in api_keys {
            let key_id = api_key_uuid(&api_key.public_id);
            let service_account_id = service_account_uuid(&api_key.service_account_key);
            let team = seeded_teams
                .get(&api_key.service_account_team_key)
                .ok_or_else(|| {
                    StoreError::NotFound(format!(
                        "seed api key `{}` references unknown team `{}`",
                        api_key.public_id, api_key.service_account_team_key
                    ))
                })?;

            self.connection
                .execute(
                    r#"
                    INSERT INTO service_accounts (
                        service_account_id, team_id, service_account_key, service_account_name,
                        status, model_access_mode, metadata_json, created_at, updated_at, disabled_at
                    ) VALUES (?1, ?2, ?3, ?4, 'active', 'all', '{}', ?5, ?5, NULL)
                    ON CONFLICT(service_account_id) DO UPDATE SET
                        team_id = excluded.team_id,
                        service_account_key = excluded.service_account_key,
                        service_account_name = excluded.service_account_name,
                        status = excluded.status,
                        model_access_mode = excluded.model_access_mode,
                        metadata_json = excluded.metadata_json,
                        updated_at = excluded.updated_at,
                        disabled_at = NULL
                    "#,
                    libsql::params![
                        service_account_id.to_string(),
                        team.team_id.to_string(),
                        api_key.service_account_key.as_str(),
                        api_key.service_account_name.as_str(),
                        now_unix
                    ],
                )
                .await
                .map_err(to_query_error)?;

            self.upsert_active_budget(
                &gateway_core::BudgetScope::ServiceAccount { service_account_id },
                &gateway_core::BudgetSettings {
                    cadence: api_key.service_account_budget.cadence,
                    amount_usd: api_key.service_account_budget.amount_usd,
                    hard_limit: api_key.service_account_budget.hard_limit,
                    timezone: api_key.service_account_budget.timezone.clone(),
                },
                now,
            )
            .await?;

            self.connection
                .execute(
                    r#"
                    INSERT INTO api_keys (
                        id, public_id, secret_hash, name, status,
                        owner_kind, owner_user_id, owner_team_id, owner_service_account_id, created_at
                    ) VALUES (?1, ?2, ?3, ?4, 'active', 'service_account', NULL, ?5, ?6, ?7)
                    ON CONFLICT(public_id) DO UPDATE SET
                        secret_hash = excluded.secret_hash,
                        name = excluded.name,
                        owner_kind = excluded.owner_kind,
                        owner_user_id = excluded.owner_user_id,
                        owner_team_id = excluded.owner_team_id,
                        owner_service_account_id = excluded.owner_service_account_id
                    "#,
                    libsql::params![
                        key_id.to_string(),
                        api_key.public_id.as_str(),
                        api_key.secret_hash.as_str(),
                        api_key.name.as_str(),
                        team.team_id.to_string(),
                        service_account_id.to_string(),
                        now_unix
                    ],
                )
                .await
                .map_err(to_query_error)?;

            self.connection
                .execute(
                    "DELETE FROM api_key_model_grants WHERE api_key_id = ?1",
                    [key_id.to_string()],
                )
                .await
                .map_err(to_query_error)?;

            for model_key in &api_key.allowed_models {
                let model_id = model_ids.get(model_key).ok_or_else(|| {
                    StoreError::NotFound(format!(
                        "seed api key `{}` references unknown model `{model_key}`",
                        api_key.public_id
                    ))
                })?;

                self.connection
                    .execute(
                        r#"
                        INSERT INTO api_key_model_grants (api_key_id, model_id)
                        VALUES (?1, ?2)
                        ON CONFLICT(api_key_id, model_id) DO NOTHING
                        "#,
                        libsql::params![key_id.to_string(), model_id.to_string()],
                    )
                    .await
                    .map_err(to_query_error)?;
            }
        }

        reconcile_seed_users(self, &seeded_teams, users, now).await?;

        Ok(())
    }
}
