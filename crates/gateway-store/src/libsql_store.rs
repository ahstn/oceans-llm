use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use gateway_core::{
    ApiKeyOwnerKind, ApiKeyRecord, ApiKeyRepository, AuthMode, BudgetCadence, BudgetRepository,
    GatewayModel, GlobalRole, IdentityRepository, MembershipRole, ModelAccessMode, ModelRepository,
    ModelRoute, Money4, ProviderConnection, ProviderRepository, RequestLogRecord,
    RequestLogRepository, StoreError, StoreHealth, TeamMembershipRecord, TeamRecord,
    UsageCostEventRecord, UserBudgetRecord, UserRecord,
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
                SELECT id, public_id, secret_hash, name, status,
                       owner_kind, owner_user_id, owner_team_id,
                       created_at, last_used_at, revoked_at
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

#[async_trait]
impl IdentityRepository for LibsqlStore {
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_id, name, email, email_normalized, global_role, auth_mode, status,
                       request_logging_enabled, model_access_mode, created_at, updated_at
                FROM users
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
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

        decode_user_record(&row).map(Some)
    }

    async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                FROM teams
                WHERE team_id = ?1
                LIMIT 1
                "#,
                [team_id.to_string()],
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

        decode_team_record(&row).map(Some)
    }

    async fn get_team_membership_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<TeamMembershipRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, user_id, role, created_at, updated_at
                FROM team_memberships
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
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

        decode_team_membership_record(&row).map(Some)
    }

    async fn list_allowed_model_keys_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        list_allowed_model_keys(
            &self.connection,
            r#"
            SELECT gm.model_key
            FROM user_model_allowlist allowlist
            INNER JOIN gateway_models gm ON gm.id = allowlist.model_id
            WHERE allowlist.user_id = ?1
            ORDER BY gm.model_key ASC
            "#,
            user_id.to_string(),
        )
        .await
    }

    async fn list_allowed_model_keys_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        list_allowed_model_keys(
            &self.connection,
            r#"
            SELECT gm.model_key
            FROM team_model_allowlist allowlist
            INNER JOIN gateway_models gm ON gm.id = allowlist.model_id
            WHERE allowlist.team_id = ?1
            ORDER BY gm.model_key ASC
            "#,
            team_id.to_string(),
        )
        .await
    }
}

#[async_trait]
impl BudgetRepository for LibsqlStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserBudgetRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                       is_active, created_at, updated_at
                FROM user_budgets
                WHERE user_id = ?1 AND is_active = 1
                LIMIT 1
                "#,
                [user_id.to_string()],
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

        decode_user_budget_record(&row).map(Some)
    }

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT COALESCE(SUM(estimated_cost_10000), 0)
                FROM usage_cost_events
                WHERE user_id = ?1
                  AND occurred_at >= ?2
                  AND occurred_at < ?3
                "#,
                libsql::params![
                    user_id.to_string(),
                    window_start.unix_timestamp(),
                    window_end.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(Money4::ZERO);
        };

        let sum_10000: i64 = row.get(0).map_err(to_query_error)?;
        Ok(Money4::from_scaled(sum_10000))
    }

    async fn insert_usage_cost_event(
        &self,
        event: &UsageCostEventRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO usage_cost_events (
                    usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                    estimated_cost_10000, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                libsql::params![
                    event.usage_event_id.to_string(),
                    event.request_id.as_str(),
                    event.api_key_id.to_string(),
                    event.user_id.map(|value| value.to_string()),
                    event.team_id.map(|value| value.to_string()),
                    event.model_id.map(|value| value.to_string()),
                    event.estimated_cost_usd.as_scaled_i64(),
                    event.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl RequestLogRepository for LibsqlStore {
    async fn insert_request_log(&self, log: &RequestLogRecord) -> Result<(), StoreError> {
        let metadata_json = serde_json::to_string(&log.metadata)
            .map_err(|error| StoreError::Serialization(error.to_string()))?;

        self.connection
            .execute(
                r#"
                INSERT INTO request_logs (
                    request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                    provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                    total_tokens, error_code, metadata_json, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
                libsql::params![
                    log.request_log_id.to_string(),
                    log.request_id.as_str(),
                    log.api_key_id.to_string(),
                    log.user_id.map(|value| value.to_string()),
                    log.team_id.map(|value| value.to_string()),
                    log.model_key.as_str(),
                    log.provider_key.as_str(),
                    log.status_code,
                    log.latency_ms,
                    log.prompt_tokens,
                    log.completion_tokens,
                    log.total_tokens,
                    log.error_code.as_deref(),
                    metadata_json,
                    log.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}

async fn list_allowed_model_keys(
    connection: &libsql::Connection,
    sql: &str,
    owner_id: String,
) -> Result<Vec<String>, StoreError> {
    let mut rows = connection
        .query(sql, [owner_id])
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?;

    let mut model_keys = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?
    {
        let model_key: String = row.get(0).map_err(to_query_error)?;
        model_keys.push(model_key);
    }

    Ok(model_keys)
}

fn decode_api_key(row: &libsql::Row) -> Result<ApiKeyRecord, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let owner_kind: String = row.get(5).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.get(7).map_err(to_query_error)?;
    let created_at: i64 = row.get(8).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.get(9).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(10).map_err(to_query_error)?;

    Ok(ApiKeyRecord {
        id: Uuid::parse_str(&id).map_err(|error| StoreError::Serialization(error.to_string()))?,
        public_id: row.get(1).map_err(to_query_error)?,
        secret_hash: row.get(2).map_err(to_query_error)?,
        name: row.get(3).map_err(to_query_error)?,
        status: row.get(4).map_err(to_query_error)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
        })?,
        owner_user_id: owner_user_id
            .as_deref()
            .map(parse_uuid)
            .transpose()
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        owner_team_id: owner_team_id
            .as_deref()
            .map(parse_uuid)
            .transpose()
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        created_at: unix_to_datetime(created_at)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_gateway_model(row: &libsql::Row) -> Result<GatewayModel, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let tags_json: String = row.get(3).map_err(to_query_error)?;

    Ok(GatewayModel {
        id: parse_uuid(&id)?,
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
        id: parse_uuid(&id)?,
        model_id: parse_uuid(&model_id)?,
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

fn decode_user_record(row: &libsql::Row) -> Result<UserRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let global_role: String = row.get(4).map_err(to_query_error)?;
    let auth_mode: String = row.get(5).map_err(to_query_error)?;
    let request_logging_enabled: i64 = row.get(7).map_err(to_query_error)?;
    let model_access_mode: String = row.get(8).map_err(to_query_error)?;
    let created_at: i64 = row.get(9).map_err(to_query_error)?;
    let updated_at: i64 = row.get(10).map_err(to_query_error)?;

    Ok(UserRecord {
        user_id: parse_uuid(&user_id)?,
        name: row.get(1).map_err(to_query_error)?,
        email: row.get(2).map_err(to_query_error)?,
        email_normalized: row.get(3).map_err(to_query_error)?,
        global_role: GlobalRole::from_db(&global_role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown global role `{global_role}`"))
        })?,
        auth_mode: AuthMode::from_db(&auth_mode)
            .ok_or_else(|| StoreError::Serialization(format!("unknown auth mode `{auth_mode}`")))?,
        status: row.get(6).map_err(to_query_error)?,
        request_logging_enabled: request_logging_enabled == 1,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_team_record(row: &libsql::Row) -> Result<TeamRecord, StoreError> {
    let team_id: String = row.get(0).map_err(to_query_error)?;
    let model_access_mode: String = row.get(4).map_err(to_query_error)?;
    let created_at: i64 = row.get(5).map_err(to_query_error)?;
    let updated_at: i64 = row.get(6).map_err(to_query_error)?;

    Ok(TeamRecord {
        team_id: parse_uuid(&team_id)?,
        team_key: row.get(1).map_err(to_query_error)?,
        team_name: row.get(2).map_err(to_query_error)?,
        status: row.get(3).map_err(to_query_error)?,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_team_membership_record(row: &libsql::Row) -> Result<TeamMembershipRecord, StoreError> {
    let team_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let role: String = row.get(2).map_err(to_query_error)?;
    let created_at: i64 = row.get(3).map_err(to_query_error)?;
    let updated_at: i64 = row.get(4).map_err(to_query_error)?;

    Ok(TeamMembershipRecord {
        team_id: parse_uuid(&team_id)?,
        user_id: parse_uuid(&user_id)?,
        role: MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_user_budget_record(row: &libsql::Row) -> Result<UserBudgetRecord, StoreError> {
    let user_budget_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let cadence: String = row.get(2).map_err(to_query_error)?;
    let amount_10000: i64 = row.get(3).map_err(to_query_error)?;
    let hard_limit: i64 = row.get(4).map_err(to_query_error)?;
    let is_active: i64 = row.get(6).map_err(to_query_error)?;
    let created_at: i64 = row.get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.get(8).map_err(to_query_error)?;

    Ok(UserBudgetRecord {
        user_budget_id: parse_uuid(&user_budget_id)?,
        user_id: parse_uuid(&user_id)?,
        cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
            StoreError::Serialization(format!("unknown budget cadence `{cadence}`"))
        })?,
        amount_usd: Money4::from_scaled(amount_10000),
        hard_limit: hard_limit == 1,
        timezone: row.get(5).map_err(to_query_error)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn json_object_from_str(value: &str) -> Result<Map<String, Value>, StoreError> {
    serde_json::from_str(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn unix_to_datetime(ts: i64) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::from_unix_timestamp(ts)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn parse_uuid(raw: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn to_query_error(error: libsql::Error) -> StoreError {
    StoreError::Query(error.to_string())
}
