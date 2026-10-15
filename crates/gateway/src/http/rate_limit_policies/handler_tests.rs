use axum::{
    http::{HeaderValue, StatusCode, header::COOKIE},
    response::IntoResponse,
};
use gateway_core::{AuthMode, GlobalRole, UserStatus};
use gateway_service::{RateLimitDecision, RateLimitSubject};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::Duration;

use super::*;

struct TestContext {
    state: AppState,
    _directory: tempfile::TempDir,
}

impl TestContext {
    async fn new() -> Self {
        let (directory, mut state) = crate::http::test_support::app_state().await;
        crate::http::test_support::install_rate_limits(
            &mut state,
            vec![ConfiguredRateLimitPolicy {
                name: "default per key".to_string(),
                operation: RateLimitOperation::Inference,
                scope_kind: RateLimitScopeKind::ApiKey,
                model_key: None,
                mcp_server_key: None,
                metric: RateLimitMetric::Requests,
                period: RateLimitPeriod::Minute,
                limit: 600,
            }],
        );
        Self {
            state,
            _directory: directory,
        }
    }

    async fn user(&self, role: GlobalRole) -> UserRecord {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "Rate limit admin",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .unwrap();
        self.state
            .store
            .get_user_by_id(user.user_id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn session(&self, user_id: Uuid) -> HeaderMap {
        let session_id = Uuid::new_v4();
        let token = format!("{session_id}.rate-limit-test");
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

    async fn admin(&self) -> HeaderMap {
        let admin = self.user(GlobalRole::PlatformAdmin).await;
        self.session(admin.user_id).await
    }

    async fn create(&self, headers: &HeaderMap, body: Value) -> Result<Value, StatusCode> {
        let request = serde_json::from_value(body).unwrap();
        match create_rate_limit_policy(State(self.state.clone()), headers.clone(), Json(request))
            .await
        {
            Ok(Json(envelope)) => Ok(serde_json::to_value(envelope.data).unwrap()),
            Err(error) => Err(error.into_response().status()),
        }
    }

    async fn list(&self, headers: &HeaderMap) -> Value {
        let Json(envelope) = list_rate_limit_policies(State(self.state.clone()), headers.clone())
            .await
            .unwrap_or_else(|error| panic!("{}", error.into_response().status()));
        serde_json::to_value(envelope.data).unwrap()
    }
}

fn user_policy(user_id: Uuid, limit: i64) -> Value {
    json!({
        "name": "one user",
        "operation": "inference",
        "scope_kind": "user",
        "scope_id": user_id.to_string(),
        "metric": "requests",
        "period": "minute",
        "limit": limit,
    })
}

#[tokio::test]
async fn platform_admins_create_update_list_and_delete_policies() {
    let context = TestContext::new().await;
    let headers = context.admin().await;
    let target = context.user(GlobalRole::User).await;

    let created = context
        .create(&headers, user_policy(target.user_id, 5))
        .await
        .unwrap();
    assert_eq!(created["source"], "admin");
    assert_eq!(created["scope_label"], target.email.as_str());
    assert_eq!(created["limit"], 5);
    let policy_id = created["id"].as_str().unwrap().to_string();

    // The new policy applies on the next check, without a restart.
    let auth = gateway_core::AuthenticatedApiKey {
        id: Uuid::new_v4(),
        public_id: "rl".to_string(),
        name: "rl".to_string(),
        model_grant_mode: gateway_core::ApiKeyModelGrantMode::All,
        owner_kind: gateway_core::ApiKeyOwnerKind::User,
        owner_user_id: Some(target.user_id),
        owner_team_id: None,
        owner_service_account_id: None,
    };
    let subject = RateLimitSubject::new(RateLimitOperation::Inference, &auth);
    let decision = context
        .state
        .service
        .rate_limiter()
        .check(&subject)
        .await
        .unwrap();
    assert!(matches!(
        decision,
        RateLimitDecision::Admitted(ref status) if status.policy_name == "one user" && status.remaining == 4
    ));

    let mut update = user_policy(target.user_id, 50);
    update["enabled"] = json!(false);
    let Json(updated) = update_rate_limit_policy(
        State(context.state.clone()),
        headers.clone(),
        Path(policy_id.clone()),
        Json(serde_json::from_value(update).unwrap()),
    )
    .await
    .unwrap_or_else(|error| panic!("{}", error.into_response().status()));
    assert_eq!(updated.data.limit, 50);
    assert!(!updated.data.enabled);

    let listed = context.list(&headers).await;
    let policies = listed["policies"].as_array().unwrap();
    assert_eq!(policies.len(), 2);
    assert_eq!(policies[0]["source"], "config");
    assert_eq!(policies[0]["name"], "default per key");
    assert!(policies[0]["id"].is_null());
    assert_eq!(policies[1]["id"], policy_id.as_str());
    // The list hydrates labels in bulk; the result matches the single view.
    assert_eq!(policies[1]["scope_label"], target.email.as_str());

    let Json(deleted) = delete_rate_limit_policy(
        State(context.state.clone()),
        headers.clone(),
        Path(policy_id.clone()),
    )
    .await
    .unwrap_or_else(|error| panic!("{}", error.into_response().status()));
    assert_eq!(deleted.data.name, "one user");
    assert_eq!(
        context.list(&headers).await["policies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let missing = delete_rate_limit_policy(State(context.state.clone()), headers, Path(policy_id))
        .await
        .err()
        .unwrap();
    assert_eq!(missing.into_response().status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_policies_are_rejected() {
    let context = TestContext::new().await;
    let headers = context.admin().await;
    let target = context.user(GlobalRole::User).await;

    let cases = [
        // Unknown principal for the scope kind.
        (user_policy(Uuid::new_v4(), 5), StatusCode::BAD_REQUEST),
        // Token limits only apply to inference.
        (
            json!({"name": "x", "operation": "mcp_tool_call", "scope_kind": "global",
                   "metric": "tokens", "period": "day", "limit": 5}),
            StatusCode::BAD_REQUEST,
        ),
        // A global policy cannot name a principal.
        (
            json!({"name": "x", "operation": "inference", "scope_kind": "global",
                   "scope_id": target.user_id.to_string(), "metric": "requests",
                   "period": "day", "limit": 5}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "x", "operation": "inference", "scope_kind": "global",
                   "model_key": "missing-model", "metric": "requests", "period": "day",
                   "limit": 5}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "x", "operation": "mcp_tool_call", "scope_kind": "api_key",
                   "mcp_server_id": Uuid::new_v4().to_string(), "metric": "requests",
                   "period": "day", "limit": 5}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"name": "x", "operation": "inference", "scope_kind": "team",
                   "metric": "requests", "period": "day", "limit": 0}),
            StatusCode::BAD_REQUEST,
        ),
        // Config names are reserved.
        (
            json!({"name": "default per key", "operation": "inference", "scope_kind": "team",
                   "metric": "requests", "period": "day", "limit": 5}),
            StatusCode::CONFLICT,
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(
            context.create(&headers, body.clone()).await.unwrap_err(),
            expected,
            "{body}"
        );
    }

    context
        .create(&headers, user_policy(target.user_id, 5))
        .await
        .unwrap();
    assert_eq!(
        context
            .create(&headers, user_policy(target.user_id, 9))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn only_platform_admins_can_read_or_change_policies() {
    let context = TestContext::new().await;
    let member = context.user(GlobalRole::User).await;
    let headers = context.session(member.user_id).await;

    let listed = list_rate_limit_policies(State(context.state.clone()), headers.clone())
        .await
        .err()
        .unwrap();
    assert_eq!(listed.into_response().status(), StatusCode::FORBIDDEN);
    assert_eq!(
        context
            .create(&headers, user_policy(member.user_id, 5))
            .await
            .unwrap_err(),
        StatusCode::FORBIDDEN
    );
    let anonymous = list_rate_limit_policies(State(context.state.clone()), HeaderMap::new())
        .await
        .err()
        .unwrap();
    assert_eq!(anonymous.into_response().status(), StatusCode::UNAUTHORIZED);
}
