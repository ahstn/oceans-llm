use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError, IdentityRepository,
    RequestLogBundle, RequestLogRepository,
};

#[derive(Clone)]
pub struct RequestLogging<R> {
    repo: Arc<R>,
}

impl<R> RequestLogging<R>
where
    R: IdentityRepository + RequestLogRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn should_log_request(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<bool, GatewayError> {
        match api_key.owner_kind {
            ApiKeyOwnerKind::Team => Ok(true),
            ApiKeyOwnerKind::User => {
                let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
                let user = self
                    .repo
                    .get_user_by_id(user_id)
                    .await?
                    .ok_or(AuthError::ApiKeyOwnerInvalid)?;
                Ok(user.request_logging_enabled)
            }
        }
    }

    pub async fn log_request_if_enabled(
        &self,
        api_key: &AuthenticatedApiKey,
        mut bundle: RequestLogBundle,
    ) -> Result<bool, GatewayError> {
        bundle.summary.user_id = api_key.owner_user_id;
        bundle.summary.team_id = api_key.owner_team_id;
        if let Some(payload) = &mut bundle.payload {
            payload.request_log_id = bundle.summary.request_log_id;
        }

        if !self.should_log_request(api_key).await? {
            return Ok(false);
        }

        self.repo.insert_request_log_bundle(&bundle).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey, GlobalRole, IdentityRepository,
        ModelAccessMode, RequestLogBundle, RequestLogPayloadRecord, RequestLogRecord,
        RequestLogRepository, StoreError, TeamMembershipRecord, TeamRecord, UserRecord,
    };
    use serde_json::Map;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::RequestLogging;

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        users: Arc<Mutex<Vec<UserRecord>>>,
        logs: Arc<Mutex<Vec<RequestLogBundle>>>,
    }

    #[async_trait]
    impl IdentityRepository for InMemoryRepo {
        async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(self
                .users
                .lock()
                .expect("users lock")
                .iter()
                .find(|user| user.user_id == user_id)
                .cloned())
        }

        async fn get_team_by_id(&self, _team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_membership_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(None)
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl RequestLogRepository for InMemoryRepo {
        async fn insert_request_log_bundle(
            &self,
            bundle: &RequestLogBundle,
        ) -> Result<(), StoreError> {
            self.logs.lock().expect("logs lock").push(bundle.clone());
            Ok(())
        }

        async fn list_request_logs(
            &self,
            _limit: usize,
        ) -> Result<Vec<RequestLogRecord>, StoreError> {
            Ok(Vec::new())
        }

        async fn get_request_log_payload_by_request_id(
            &self,
            _request_id: &str,
        ) -> Result<Option<RequestLogPayloadRecord>, StoreError> {
            Ok(None)
        }
    }

    fn user_record(user_id: Uuid, request_logging_enabled: bool) -> UserRecord {
        UserRecord {
            user_id,
            name: "test".to_string(),
            email: "user@example.com".to_string(),
            email_normalized: "user@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            status: "active".to_string(),
            must_change_password: false,
            request_logging_enabled,
            model_access_mode: ModelAccessMode::All,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    fn sample_log(api_key_id: Uuid) -> RequestLogRecord {
        RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req_1".to_string(),
            api_key_id,
            user_id: None,
            team_id: None,
            model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            status_code: Some(200),
            latency_ms: Some(120),
            stream: false,
            fallback_used: false,
            attempt_count: 1,
            prompt_tokens: Some(100),
            completion_tokens: Some(200),
            total_tokens: Some(300),
            payload_available: false,
            error_code: None,
            metadata: Map::new(),
            occurred_at: OffsetDateTime::now_utc(),
        }
    }

    #[tokio::test]
    async fn suppresses_logging_for_user_toggle_disabled() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo {
            users: Arc::new(Mutex::new(vec![user_record(user_id, false)])),
            logs: Arc::new(Mutex::new(Vec::new())),
        });
        let logging = RequestLogging::new(repo.clone());
        let api_key_id = Uuid::new_v4();
        let auth = AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
        };

        let wrote = logging
            .log_request_if_enabled(
                &auth,
                RequestLogBundle {
                    summary: sample_log(api_key_id),
                    payload: None,
                },
            )
            .await
            .expect("request logging should evaluate");

        assert!(!wrote);
        assert_eq!(repo.logs.lock().expect("logs lock").len(), 0);
    }

    #[tokio::test]
    async fn logs_team_owned_requests_with_nullable_user() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new(repo.clone());
        let api_key_id = Uuid::new_v4();
        let auth = AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        };

        let wrote = logging
            .log_request_if_enabled(
                &auth,
                RequestLogBundle {
                    summary: sample_log(api_key_id),
                    payload: None,
                },
            )
            .await
            .expect("request logging should evaluate");

        let logs = repo.logs.lock().expect("logs lock");
        assert!(wrote);
        assert_eq!(logs.len(), 1);
        assert!(logs[0].summary.user_id.is_none());
        assert_eq!(logs[0].summary.team_id, Some(team_id));
    }
}
