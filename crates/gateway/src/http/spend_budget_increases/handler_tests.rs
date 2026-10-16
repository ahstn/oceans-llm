use axum::{
    body::to_bytes,
    http::{HeaderValue, StatusCode, header::COOKIE},
    response::IntoResponse,
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, BudgetCadence,
    BudgetSettings, GlobalRole, MembershipRole, NewApiKeyRecord, UsageLedgerRecord,
    UsagePricingStatus, UserStatus,
};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::http::admin_contract::BudgetIncreasePeriod;

struct TestContext {
    state: AppState,
    _directory: tempfile::TempDir,
    team_a: Uuid,
    team_b: Uuid,
}

impl TestContext {
    async fn new() -> Self {
        let (directory, state) = crate::http::test_support::app_state().await;
        let team_a = state.store.create_team("team-a", "Team A").await.unwrap();
        let team_b = state.store.create_team("team-b", "Team B").await.unwrap();
        Self {
            state,
            _directory: directory,
            team_a: team_a.team_id,
            team_b: team_b.team_id,
        }
    }

    async fn user(&self, role: GlobalRole, team: Option<(Uuid, MembershipRole)>) -> Uuid {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "Budget user",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .unwrap();
        if let Some((team_id, role)) = team {
            self.state
                .store
                .assign_team_membership(user.user_id, team_id, role)
                .await
                .unwrap();
        }
        user.user_id
    }

    async fn session(&self, user_id: Uuid) -> HeaderMap {
        let session_id = Uuid::new_v4();
        let token = format!("{session_id}.budget-increase-test");
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let now = OffsetDateTime::now_utc();
        self.state
            .store
            .create_user_session(
                session_id,
                user_id,
                &token_hash,
                now + Duration::hours(1),
                now,
            )
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("ogw_session={token}")).unwrap(),
        );
        headers
    }

    /// A $100 monthly budget with `spend_10000` already recorded this month.
    async fn budgeted_user(&self, team_id: Uuid, spend_10000: i64) -> Uuid {
        let user_id = self
            .user(GlobalRole::User, Some((team_id, MembershipRole::Member)))
            .await;
        self.state
            .store
            .upsert_active_budget(
                &BudgetScope::User { user_id },
                &BudgetSettings {
                    cadence: BudgetCadence::Monthly,
                    amount_usd: Money4::from_scaled(1_000_000),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                },
                OffsetDateTime::now_utc(),
            )
            .await
            .unwrap();
        let key = self
            .state
            .store
            .create_api_key(&NewApiKeyRecord {
                name: "Budget test".into(),
                public_id: Uuid::new_v4().simple().to_string(),
                secret_hash: gateway_core::hash_gateway_key_secret("test-secret").unwrap(),
                model_grant_mode: ApiKeyModelGrantMode::Explicit,
                owner_kind: ApiKeyOwnerKind::User,
                owner_user_id: Some(user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                created_at: OffsetDateTime::now_utc(),
            })
            .await
            .unwrap();
        self.state
            .store
            .insert_usage_ledger_if_absent(&ledger(key.id, user_id, team_id, spend_10000))
            .await
            .unwrap();
        user_id
    }

    async fn increase(
        &self,
        headers: HeaderMap,
        user_id: Uuid,
        amount_usd: &str,
    ) -> (StatusCode, Value) {
        let response = create_budget_increase(
            State(self.state.clone()),
            headers,
            crate::http::audit::AuditRequestMeta::default(),
            Json(CreateBudgetIncreaseRequest {
                user_id: user_id.to_string(),
                amount_usd: amount_usd.to_string(),
                period: BudgetIncreasePeriod::SevenDays,
            }),
        )
        .await
        .into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }
}

fn ledger(api_key_id: Uuid, user_id: Uuid, team_id: Uuid, cost_10000: i64) -> UsageLedgerRecord {
    // Mid-window: a ledger row at `now` could land after the listing's window end.
    let occurred_at = OffsetDateTime::now_utc() - Duration::seconds(5);
    UsageLedgerRecord {
        usage_event_id: Uuid::new_v4(),
        request_id: format!("req_{}", Uuid::new_v4().simple()),
        ownership_scope_key: format!("user:{user_id}"),
        api_key_id,
        user_id: Some(user_id),
        team_id: Some(team_id),
        service_account_id: None,
        actor_user_id: None,
        model_id: None,
        model_route_id: None,
        provider_key: "openai-prod".to_string(),
        upstream_model: "gpt-4o-mini".to_string(),
        prompt_tokens: Some(100),
        uncached_input_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        completion_tokens: Some(50),
        total_tokens: Some(150),
        provider_usage: json!({}),
        pricing_status: UsagePricingStatus::Priced,
        unpriced_reason: None,
        pricing_row_id: None,
        pricing_provider_id: None,
        pricing_model_id: None,
        pricing_source: None,
        pricing_source_etag: None,
        pricing_source_fetched_at: None,
        pricing_last_updated: None,
        input_cost_per_million_tokens: None,
        output_cost_per_million_tokens: None,
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
        computed_cost_usd: Money4::from_scaled(cost_10000),
        occurred_at,
    }
}

#[tokio::test]
async fn team_admin_can_increase_only_their_own_team_members() {
    let ctx = TestContext::new().await;
    let near_limit = ctx.budgeted_user(ctx.team_a, 900_000).await;
    let other_team = ctx.budgeted_user(ctx.team_b, 900_000).await;
    let team_admin = ctx
        .user(GlobalRole::User, Some((ctx.team_a, MembershipRole::Admin)))
        .await;
    let member = ctx
        .user(GlobalRole::User, Some((ctx.team_a, MembershipRole::Member)))
        .await;

    let (status, body) = ctx
        .increase(ctx.session(team_admin).await, near_limit, "50")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["effective_amount_usd_10000"], 1_500_000);
    assert_eq!(body["data"]["increase"]["amount_usd_10000"], 500_000);

    let (status, _) = ctx
        .increase(ctx.session(team_admin).await, other_team, "50")
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = ctx
        .increase(ctx.session(member).await, near_limit, "50")
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let platform_admin = ctx.user(GlobalRole::PlatformAdmin, None).await;
    let (status, body) = ctx
        .increase(ctx.session(platform_admin).await, other_team, "300")
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn increase_requires_threshold_and_respects_cap() {
    let ctx = TestContext::new().await;
    let admin = ctx.user(GlobalRole::PlatformAdmin, None).await;
    let headers = ctx.session(admin).await;
    let below_threshold = ctx.budgeted_user(ctx.team_a, 840_000).await;
    let at_threshold = ctx.budgeted_user(ctx.team_a, 850_000).await;

    let (status, body) = ctx.increase(headers.clone(), below_threshold, "50").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, _) = ctx
        .increase(headers.clone(), at_threshold, "300.0001")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = ctx.increase(headers.clone(), at_threshold, "0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = ctx.increase(headers.clone(), at_threshold, "300").await;
    assert_eq!(status, StatusCode::OK);

    // $85 of a $400 effective limit is under the threshold, so no stacking.
    let (status, _) = ctx.increase(headers, at_threshold, "50").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn team_admin_listing_is_limited_to_their_team() {
    let ctx = TestContext::new().await;
    let near_limit = ctx.budgeted_user(ctx.team_a, 900_000).await;
    ctx.budgeted_user(ctx.team_b, 900_000).await;
    let team_admin = ctx
        .user(GlobalRole::User, Some((ctx.team_a, MembershipRole::Owner)))
        .await;
    let headers = ctx.session(team_admin).await;
    let (status, _) = ctx.increase(headers.clone(), near_limit, "25").await;
    assert_eq!(status, StatusCode::OK);

    let Json(listing) = crate::http::spend::list_spend_budgets(State(ctx.state.clone()), headers)
        .await
        .unwrap_or_else(|error| panic!("listing failed: {:?}", error.0));
    let listing = serde_json::to_value(listing).unwrap();
    let users = listing["data"]["users"].as_array().unwrap();
    let team_a = ctx.team_a.to_string();
    assert!(
        users
            .iter()
            .all(|user| user["team_id"].as_str() == Some(team_a.as_str()))
    );
    let target = users
        .iter()
        .find(|user| user["user_id"] == near_limit.to_string())
        .unwrap();
    assert_eq!(target["active_increase"]["amount_usd_10000"], 250_000);
    assert_eq!(target["current_window_spend_usd_10000"], 900_000);
}
