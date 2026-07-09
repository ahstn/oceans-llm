use super::*;
use crate::seed::{
    apply_human_budget_defaults_for_user, generate_seed_api_key_material, managed_api_key_uuid,
    prevalidate_seed_users, provided_seed_api_key_material, reconcile_human_budget_defaults,
    reconcile_seed_teams, reconcile_seed_users, service_account_uuid,
    validate_seed_service_account_team_references,
};
use crate::shared::{parse_uuid, serialize_json, serialize_optional_json};

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
        service_accounts: &[gateway_core::SeedServiceAccount],
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
            let allowed_email_domains_json = serialize_json(&provider.allowed_email_domains)?;
            let oauth_provider_id = crate::seed::oauth_provider_uuid(&provider.provider_key);
            self.connection
                .execute(
                    r#"
                    INSERT INTO oauth_providers (
                        oauth_provider_id, provider_key, provider_type, client_id,
                        scopes_json, enabled, label, client_secret_ref, jit_enabled,
                        jit_global_role, jit_team_key, jit_team_role,
                        jit_request_logging_enabled, allowed_email_domains_json,
                        sso_email_verification_enabled, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
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
                        allowed_email_domains_json = excluded.allowed_email_domains_json,
                        sso_email_verification_enabled = excluded.sso_email_verification_enabled,
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
                        allowed_email_domains_json,
                        if provider.sso_email_verification_enabled {
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

            self.connection
                .execute(
                    "DELETE FROM model_allowlist_users WHERE model_id = ?1",
                    [model_id.to_string()],
                )
                .await
                .map_err(to_query_error)?;
            self.connection
                .execute(
                    "DELETE FROM model_allowlist_teams WHERE model_id = ?1",
                    [model_id.to_string()],
                )
                .await
                .map_err(to_query_error)?;

            if let Some(allowlist) = &model.allowlist {
                for normalized_email in &allowlist.users {
                    self.connection
                        .execute(
                            "INSERT INTO model_allowlist_users (model_id, normalized_email) VALUES (?1, ?2) ON CONFLICT(model_id, normalized_email) DO NOTHING",
                            libsql::params![model_id.to_string(), normalized_email.as_str()],
                        )
                        .await
                        .map_err(to_query_error)?;
                }
                for team_key in &allowlist.teams {
                    self.connection
                        .execute(
                            "INSERT INTO model_allowlist_teams (model_id, team_key) VALUES (?1, ?2) ON CONFLICT(model_id, team_key) DO NOTHING",
                            libsql::params![model_id.to_string(), team_key.as_str()],
                        )
                        .await
                        .map_err(to_query_error)?;
                }
            }

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

        validate_seed_service_account_team_references(teams, service_accounts)?;
        prevalidate_seed_users(self, users).await?;
        let seeded_teams = reconcile_seed_teams(self, teams, now).await?;

        let mut seeded_service_accounts = std::collections::BTreeMap::new();
        for service_account in service_accounts {
            let service_account_id = service_account_uuid(&service_account.service_account_key);
            let team = seeded_teams.get(&service_account.team_key).ok_or_else(|| {
                StoreError::NotFound(format!(
                    "seed service account `{}` references unknown team `{}`",
                    service_account.service_account_key, service_account.team_key
                ))
            })?;

            if let Some(existing) =
                Self::get_service_account_by_id(self, service_account_id).await?
                && existing.team_id != team.team_id
            {
                return Err(StoreError::Conflict(format!(
                    "seed service account '{}' cannot change team",
                    service_account.service_account_key
                )));
            }
            if let Some(tags) = service_account.tags.as_ref() {
                let tags_json = serialize_json(tags)?;
                self.connection
                    .execute(
                        r#"
                        INSERT INTO service_accounts (
                            service_account_id, team_id, service_account_key, service_account_name,
                            status, model_access_mode, metadata_json, tags_json, created_at, updated_at, disabled_at
                        ) VALUES (?1, ?2, ?3, ?4, 'active', 'all', '{}', ?5, ?6, ?6, NULL)
                        ON CONFLICT(service_account_id) DO UPDATE SET
                            service_account_key = excluded.service_account_key,
                            service_account_name = excluded.service_account_name,
                            tags_json = excluded.tags_json,
                            updated_at = excluded.updated_at
                        "#,
                        libsql::params![
                            service_account_id.to_string(),
                            team.team_id.to_string(),
                            service_account.service_account_key.as_str(),
                            service_account.service_account_name.as_str(),
                            tags_json,
                            now_unix
                        ],
                    )
                    .await
                    .map_err(to_query_error)?;
            } else {
                self.connection
                    .execute(
                        r#"
                        INSERT INTO service_accounts (
                            service_account_id, team_id, service_account_key, service_account_name,
                            status, model_access_mode, metadata_json, created_at, updated_at, disabled_at
                        ) VALUES (?1, ?2, ?3, ?4, 'active', 'all', '{}', ?5, ?5, NULL)
                        ON CONFLICT(service_account_id) DO UPDATE SET
                            service_account_key = excluded.service_account_key,
                            service_account_name = excluded.service_account_name,
                            updated_at = excluded.updated_at
                        "#,
                        libsql::params![
                            service_account_id.to_string(),
                            team.team_id.to_string(),
                            service_account.service_account_key.as_str(),
                            service_account.service_account_name.as_str(),
                            now_unix
                        ],
                    )
                    .await
                    .map_err(to_query_error)?;
            }

            self.upsert_active_budget(
                &gateway_core::BudgetScope::ServiceAccount { service_account_id },
                &gateway_core::BudgetSettings {
                    cadence: service_account.budget.cadence,
                    amount_usd: service_account.budget.amount_usd,
                    hard_limit: service_account.budget.hard_limit,
                    timezone: service_account.budget.timezone.clone(),
                },
                now,
            )
            .await?;

            for managed_key in &service_account.managed_api_keys {
                let mut rows = self
                    .connection
                    .query(
                        r#"
                        SELECT api_key_id
                        FROM api_key_managed_credentials
                        WHERE service_account_id = ?1
                          AND config_key = ?2
                        LIMIT 1
                        "#,
                        libsql::params![
                            service_account_id.to_string(),
                            managed_key.config_key.as_str()
                        ],
                    )
                    .await
                    .map_err(to_query_error)?;

                let existing_managed_api_key_id = rows
                    .next()
                    .await
                    .map_err(to_query_error)?
                    .map(|row| {
                        let raw: String = row.get(0).map_err(to_query_error)?;
                        parse_uuid(&raw)
                    })
                    .transpose()?;
                let is_existing_managed_key = existing_managed_api_key_id.is_some();
                let should_apply_secret = managed_key.source
                    == gateway_core::ManagedApiKeySource::ConfiguredValue
                    || !is_existing_managed_key;

                let mut secret_material_to_apply = None;
                let api_key_id = if let Some(api_key_id) = existing_managed_api_key_id {
                    let existing_api_key =
                        AdminApiKeyRepository::get_api_key_by_id(self, api_key_id)
                            .await?
                            .ok_or_else(|| {
                                StoreError::NotFound(format!(
                                    "managed api key `{}` points to missing api key `{api_key_id}`",
                                    managed_key.config_key
                                ))
                            })?;
                    let provided_material = provided_seed_api_key_material(managed_key)?;
                    let is_rotation = should_apply_secret
                        && managed_key.source == gateway_core::ManagedApiKeySource::ConfiguredValue
                        && provided_material.as_ref().is_some_and(|(public_id, _, _)| {
                            existing_api_key.public_id != *public_id
                        });

                    if is_rotation {
                        let (public_id, secret_hash, secret_material) = provided_material
                            .ok_or_else(|| {
                                StoreError::Conflict(format!(
                                    "managed api key `{}` cannot rotate without secret material",
                                    managed_key.config_key
                                ))
                            })?;
                        let rotated_api_key_id = api_key_uuid(&public_id);
                        if AdminApiKeyRepository::get_api_key_by_id(self, rotated_api_key_id)
                            .await?
                            .is_some()
                        {
                            return Err(StoreError::Conflict(format!(
                                "managed api key `{}` public id already exists",
                                managed_key.config_key
                            )));
                        }

                        self.connection
                            .execute(
                                r#"
                                INSERT INTO api_keys (
                                    id, public_id, secret_hash, name, status,
                                    owner_kind, owner_user_id, owner_team_id, owner_service_account_id, created_at
                                ) VALUES (?1, ?2, ?3, ?4, 'active', 'service_account', NULL, ?5, ?6, ?7)
                                "#,
                                libsql::params![
                                    rotated_api_key_id.to_string(),
                                    public_id.as_str(),
                                    secret_hash.as_str(),
                                    managed_key.name.as_str(),
                                    team.team_id.to_string(),
                                    service_account_id.to_string(),
                                    now_unix
                                ],
                            )
                            .await
                            .map_err(to_query_error)?;

                        self.connection
                            .execute(
                                r#"
                                UPDATE api_keys
                                SET status = 'revoked', revoked_at = ?1
                                WHERE id = ?2
                                "#,
                                libsql::params![now_unix, api_key_id.to_string()],
                            )
                            .await
                            .map_err(to_query_error)?;

                        secret_material_to_apply = Some(secret_material);
                        rotated_api_key_id
                    } else {
                        let secret_hash = if should_apply_secret {
                            if let Some((_, secret_hash, secret_material)) = provided_material {
                                secret_material_to_apply = Some(secret_material);
                                secret_hash
                            } else {
                                existing_api_key.secret_hash.clone()
                            }
                        } else {
                            existing_api_key.secret_hash.clone()
                        };

                        self.connection
                            .execute(
                                r#"
                                UPDATE api_keys
                                SET secret_hash = ?1,
                                    name = ?2,
                                    owner_kind = 'service_account',
                                    owner_user_id = NULL,
                                    owner_team_id = ?3,
                                    owner_service_account_id = ?4
                                WHERE id = ?5
                                "#,
                                libsql::params![
                                    secret_hash.as_str(),
                                    managed_key.name.as_str(),
                                    team.team_id.to_string(),
                                    service_account_id.to_string(),
                                    api_key_id.to_string()
                                ],
                            )
                            .await
                            .map_err(to_query_error)?;

                        api_key_id
                    }
                } else {
                    let (public_id, secret_hash, secret_material) =
                        match provided_seed_api_key_material(managed_key)? {
                            Some(material) => material,
                            None if managed_key.source
                                == gateway_core::ManagedApiKeySource::Generated
                                && managed_key.auto_create =>
                            {
                                generate_seed_api_key_material()?
                            }
                            None => {
                                return Err(StoreError::Conflict(format!(
                                    "managed api key `{}` cannot be created without secret material",
                                    managed_key.config_key
                                )));
                            }
                        };
                    let api_key_id = api_key_uuid(&public_id);

                    if AdminApiKeyRepository::get_api_key_by_id(self, api_key_id)
                        .await?
                        .is_some()
                    {
                        return Err(StoreError::Conflict(format!(
                            "managed api key `{}` public id already exists",
                            managed_key.config_key
                        )));
                    }

                    self.connection
                        .execute(
                            r#"
                            INSERT INTO api_keys (
                                id, public_id, secret_hash, name, status,
                                owner_kind, owner_user_id, owner_team_id, owner_service_account_id, created_at
                            ) VALUES (?1, ?2, ?3, ?4, 'active', 'service_account', NULL, ?5, ?6, ?7)
                            "#,
                            libsql::params![
                                api_key_id.to_string(),
                                public_id.as_str(),
                                secret_hash.as_str(),
                                managed_key.name.as_str(),
                                team.team_id.to_string(),
                                service_account_id.to_string(),
                                now_unix
                            ],
                        )
                        .await
                        .map_err(to_query_error)?;

                    secret_material_to_apply = Some(secret_material);
                    api_key_id
                };

                self.connection
                    .execute(
                        r#"
                        INSERT INTO api_key_managed_credentials (
                            managed_credential_id, service_account_id, config_key, api_key_id,
                            source, auto_create, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                        ON CONFLICT(service_account_id, config_key) DO UPDATE SET
                            api_key_id = excluded.api_key_id,
                            source = excluded.source,
                            auto_create = excluded.auto_create,
                            updated_at = excluded.updated_at
                        "#,
                        libsql::params![
                            managed_api_key_uuid(
                                &service_account.service_account_key,
                                &managed_key.config_key
                            )
                            .to_string(),
                            service_account_id.to_string(),
                            managed_key.config_key.as_str(),
                            api_key_id.to_string(),
                            managed_key.source.as_str(),
                            if managed_key.auto_create {
                                1_i64
                            } else {
                                0_i64
                            },
                            now_unix
                        ],
                    )
                    .await
                    .map_err(to_query_error)?;

                if let Some(secret_material) = secret_material_to_apply {
                    AdminApiKeyRepository::upsert_api_key_secret_material(
                        self,
                        &gateway_core::ApiKeySecretMaterialRecord {
                            api_key_id,
                            storage_kind: secret_material.storage_kind,
                            secret_ciphertext: secret_material.secret_ciphertext.clone(),
                            secret_nonce: secret_material.secret_nonce.clone(),
                            secret_key_id: secret_material.secret_key_id.clone(),
                            created_at: now,
                            updated_at: now,
                            last_retrieved_at: None,
                        },
                    )
                    .await?;
                }

                self.connection
                    .execute(
                        "DELETE FROM api_key_model_grants WHERE api_key_id = ?1",
                        [api_key_id.to_string()],
                    )
                    .await
                    .map_err(to_query_error)?;

                for model_key in &managed_key.allowed_models {
                    let model_id = model_ids.get(model_key).ok_or_else(|| {
                        StoreError::NotFound(format!(
                            "managed api key `{}` references unknown model `{model_key}`",
                            managed_key.config_key
                        ))
                    })?;

                    self.connection
                        .execute(
                            r#"
                            INSERT INTO api_key_model_grants (api_key_id, model_id)
                            VALUES (?1, ?2)
                            ON CONFLICT(api_key_id, model_id) DO NOTHING
                            "#,
                            libsql::params![api_key_id.to_string(), model_id.to_string()],
                        )
                        .await
                        .map_err(to_query_error)?;
                }
            }

            if let Some(record) = Self::get_service_account_by_id(self, service_account_id).await? {
                seeded_service_accounts.insert(service_account.service_account_key.clone(), record);
            }
        }

        for api_key in api_keys {
            let key_id = api_key_uuid(&api_key.public_id);
            let service_account = seeded_service_accounts
                .get(&api_key.service_account_key)
                .ok_or_else(|| {
                    StoreError::NotFound(format!(
                        "seed api key `{}` references unknown service account `{}`",
                        api_key.public_id, api_key.service_account_key
                    ))
                })?;

            self.connection
                .execute(
                    r#"
                    INSERT INTO api_keys (
                        id, public_id, secret_hash, name, status,
                        model_grant_mode, owner_kind, owner_user_id, owner_team_id,
                        owner_service_account_id, created_at
                    ) VALUES (?1, ?2, ?3, ?4, 'active', 'explicit', 'service_account', NULL, ?5, ?6, ?7)
                    ON CONFLICT(public_id) DO UPDATE SET
                        secret_hash = excluded.secret_hash,
                        name = excluded.name,
                        model_grant_mode = excluded.model_grant_mode,
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
                        service_account.team_id.to_string(),
                        service_account.service_account_id.to_string(),
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

    pub async fn reconcile_human_budget_defaults(
        &self,
        defaults: &gateway_core::SeedHumanBudgetDefaults,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        reconcile_human_budget_defaults(self, defaults, updated_at).await
    }

    pub async fn apply_human_budget_defaults_for_user(
        &self,
        defaults: &gateway_core::SeedHumanBudgetDefaults,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        apply_human_budget_defaults_for_user(self, defaults, user_id, updated_at).await
    }
}
