use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AuthError, AuthenticatedApiKey, GatewayError, GatewayModel, IdentityRepository,
    ModelAccessMode, ModelRepository, RouteError,
};
use itertools::Itertools;

#[derive(Clone)]
pub struct ModelAccess<R> {
    repo: Arc<R>,
}

impl<R> ModelAccess<R>
where
    R: ModelRepository + IdentityRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_models_for_api_key(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        self.effective_models_for_api_key(api_key).await
    }

    pub async fn resolve_requested_model(
        &self,
        api_key: &AuthenticatedApiKey,
        requested_model: &str,
    ) -> Result<GatewayModel, GatewayError> {
        if let Some(tag_expression) = requested_model.strip_prefix("tag:") {
            return self.resolve_tag_expression(api_key, tag_expression).await;
        }

        let model = self
            .repo
            .get_model_by_key(requested_model)
            .await?
            .ok_or_else(|| RouteError::ModelNotFound(requested_model.to_string()))?;

        let effective_models = self.effective_models_for_api_key(api_key).await?;
        let has_grant = effective_models
            .iter()
            .any(|granted| granted.model_key == requested_model);

        if !has_grant {
            return Err(AuthError::ModelNotGranted(requested_model.to_string()).into());
        }

        Ok(model)
    }

    async fn resolve_tag_expression(
        &self,
        api_key: &AuthenticatedApiKey,
        tag_expression: &str,
    ) -> Result<GatewayModel, GatewayError> {
        let requested_tags = tag_expression
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToString::to_string)
            .collect_vec();

        if requested_tags.is_empty() {
            return Err(GatewayError::InvalidRequest(
                "tag expression must include at least one tag".to_string(),
            ));
        }

        let effective_models = self.effective_models_for_api_key(api_key).await?;

        effective_models
            .into_iter()
            .filter(|model| {
                requested_tags
                    .iter()
                    .all(|requested_tag| model.tags.iter().any(|tag| tag == requested_tag))
            })
            .sorted_by(|left, right| {
                left.rank
                    .cmp(&right.rank)
                    .then(left.model_key.cmp(&right.model_key))
            })
            .next()
            .ok_or_else(|| RouteError::ModelNotFound(format!("tag:{tag_expression}")).into())
    }

    async fn effective_models_for_api_key(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
        let mut allowed_model_keys: Option<HashSet<String>> = None;
        let mut effective_team_id = api_key.owner_team_id;

        if effective_team_id.is_none()
            && let Some(user_id) = api_key.owner_user_id
        {
            effective_team_id = self
                .repo
                .get_team_membership_for_user(user_id)
                .await?
                .map(|membership| membership.team_id);
        }

        if let Some(team_id) = effective_team_id {
            let team = self
                .repo
                .get_team_by_id(team_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if team.model_access_mode == ModelAccessMode::Restricted {
                let allowed_for_team = self
                    .repo
                    .list_allowed_model_keys_for_team(team_id)
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                allowed_model_keys = intersect_allowed(allowed_model_keys, allowed_for_team);
            }
        }

        if let Some(user_id) = api_key.owner_user_id {
            let user = self
                .repo
                .get_user_by_id(user_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if user.model_access_mode == ModelAccessMode::Restricted {
                let allowed_for_user = self
                    .repo
                    .list_allowed_model_keys_for_user(user_id)
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                allowed_model_keys = intersect_allowed(allowed_model_keys, allowed_for_user);
            }
        }

        let effective_models = granted_models
            .into_iter()
            .filter(|model| match &allowed_model_keys {
                Some(allowed) => allowed.contains(&model.model_key),
                None => true,
            })
            .sorted_by(|left, right| {
                left.rank
                    .cmp(&right.rank)
                    .then(left.model_key.cmp(&right.model_key))
            })
            .collect::<Vec<_>>();

        Ok(effective_models)
    }
}

fn intersect_allowed(
    current: Option<HashSet<String>>,
    next: HashSet<String>,
) -> Option<HashSet<String>> {
    match current {
        None => Some(next),
        Some(existing) => Some(existing.intersection(&next).cloned().collect()),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey, GatewayError, GatewayModel, GlobalRole,
        IdentityRepository, MembershipRole, ModelAccessMode, ModelRepository, ModelRoute,
        StoreError, TeamMembershipRecord, TeamRecord, UserRecord,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::ModelAccess;

    #[derive(Clone, Default)]
    struct InMemoryModelRepo {
        models: HashMap<String, GatewayModel>,
        grants: HashMap<Uuid, Vec<String>>,
        users: HashMap<Uuid, UserRecord>,
        teams: HashMap<Uuid, TeamRecord>,
        memberships: HashMap<Uuid, TeamMembershipRecord>,
        user_allowlist: HashMap<Uuid, Vec<String>>,
        team_allowlist: HashMap<Uuid, Vec<String>>,
    }

    #[async_trait]
    impl ModelRepository for InMemoryModelRepo {
        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self.models.get(model_key).cloned())
        }

        async fn list_models_for_api_key(
            &self,
            api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            let model_keys = self.grants.get(&api_key_id).cloned().unwrap_or_default();
            Ok(model_keys
                .into_iter()
                .filter_map(|model_key| self.models.get(&model_key).cloned())
                .collect())
        }

        async fn list_routes_for_model(
            &self,
            _model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl IdentityRepository for InMemoryModelRepo {
        async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(self.users.get(&user_id).cloned())
        }

        async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(self.teams.get(&team_id).cloned())
        }

        async fn get_team_membership_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(self.memberships.get(&user_id).cloned())
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(self
                .user_allowlist
                .get(&user_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(self
                .team_allowlist
                .get(&team_id)
                .cloned()
                .unwrap_or_default())
        }
    }

    fn model(model_key: &str, tags: &[&str], rank: i32) -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: model_key.to_string(),
            description: None,
            tags: tags.iter().map(ToString::to_string).collect(),
            rank,
        }
    }

    fn user(user_id: Uuid, model_access_mode: ModelAccessMode) -> UserRecord {
        UserRecord {
            user_id,
            name: "test".to_string(),
            email: "user@example.com".to_string(),
            email_normalized: "user@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            status: "active".to_string(),
            must_change_password: false,
            request_logging_enabled: true,
            model_access_mode,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    fn team(team_id: Uuid, model_access_mode: ModelAccessMode) -> TeamRecord {
        TeamRecord {
            team_id,
            team_key: "team-key".to_string(),
            team_name: "Team".to_string(),
            status: "active".to_string(),
            model_access_mode,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    fn auth(
        api_key_id: Uuid,
        owner_kind: ApiKeyOwnerKind,
        owner_user_id: Option<Uuid>,
        owner_team_id: Option<Uuid>,
    ) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind,
            owner_user_id,
            owner_team_id,
        }
    }

    #[tokio::test]
    async fn resolves_tag_expression_by_rank_then_model_key() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let fast = model("fast", &["fast", "cheap"], 20);
        let fast_alt = model("fast-alt", &["fast", "cheap"], 10);

        let mut repo = InMemoryModelRepo::default();
        repo.models.insert(fast.model_key.clone(), fast);
        repo.models.insert(fast_alt.model_key.clone(), fast_alt);
        repo.grants
            .insert(api_key_id, vec!["fast".to_string(), "fast-alt".to_string()]);
        repo.teams
            .insert(team_id, team(team_id, ModelAccessMode::All));

        let access = ModelAccess::new(Arc::new(repo));
        let auth = auth(api_key_id, ApiKeyOwnerKind::Team, None, Some(team_id));

        let resolved = access
            .resolve_requested_model(&auth, "tag:fast,cheap")
            .await
            .expect("tag resolution should succeed");

        assert_eq!(resolved.model_key, "fast-alt");
    }

    #[tokio::test]
    async fn rejects_ungranted_model() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let reasoning = model("reasoning", &["reasoning"], 1);

        let mut repo = InMemoryModelRepo::default();
        repo.models.insert(reasoning.model_key.clone(), reasoning);
        repo.grants.insert(api_key_id, vec![]);
        repo.teams
            .insert(team_id, team(team_id, ModelAccessMode::All));

        let access = ModelAccess::new(Arc::new(repo));
        let auth = auth(api_key_id, ApiKeyOwnerKind::Team, None, Some(team_id));

        let error = access
            .resolve_requested_model(&auth, "reasoning")
            .await
            .expect_err("must reject ungranted model");

        assert!(matches!(error, GatewayError::Auth(_)));
    }

    #[tokio::test]
    async fn applies_model_restriction_intersection() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let m1 = model("fast", &["fast"], 1);
        let m2 = model("reasoning", &["reasoning"], 2);
        let m3 = model("vision", &["vision"], 3);

        let mut repo = InMemoryModelRepo::default();
        repo.models.insert(m1.model_key.clone(), m1);
        repo.models.insert(m2.model_key.clone(), m2);
        repo.models.insert(m3.model_key.clone(), m3);
        repo.grants.insert(
            api_key_id,
            vec![
                "fast".to_string(),
                "reasoning".to_string(),
                "vision".to_string(),
            ],
        );
        repo.teams
            .insert(team_id, team(team_id, ModelAccessMode::Restricted));
        repo.users
            .insert(user_id, user(user_id, ModelAccessMode::Restricted));
        repo.memberships.insert(
            user_id,
            TeamMembershipRecord {
                team_id,
                user_id,
                role: MembershipRole::Member,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            },
        );
        repo.team_allowlist
            .insert(team_id, vec!["fast".to_string(), "reasoning".to_string()]);
        repo.user_allowlist
            .insert(user_id, vec!["reasoning".to_string(), "vision".to_string()]);

        let access = ModelAccess::new(Arc::new(repo));
        let auth = auth(api_key_id, ApiKeyOwnerKind::User, Some(user_id), None);

        let models = access
            .list_models_for_api_key(&auth)
            .await
            .expect("effective model listing must succeed");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_key, "reasoning");
    }

    #[tokio::test]
    async fn supports_all_mode_without_allowlists() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let m1 = model("fast", &["fast"], 1);
        let m2 = model("reasoning", &["reasoning"], 2);

        let mut repo = InMemoryModelRepo::default();
        repo.models.insert(m1.model_key.clone(), m1);
        repo.models.insert(m2.model_key.clone(), m2);
        repo.grants.insert(
            api_key_id,
            vec!["fast".to_string(), "reasoning".to_string()],
        );
        repo.teams
            .insert(team_id, team(team_id, ModelAccessMode::All));
        repo.users
            .insert(user_id, user(user_id, ModelAccessMode::All));

        let access = ModelAccess::new(Arc::new(repo));
        let auth = auth(api_key_id, ApiKeyOwnerKind::User, Some(user_id), None);

        let models = access
            .list_models_for_api_key(&auth)
            .await
            .expect("effective model listing must succeed");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_key, "fast");
        assert_eq!(models[1].model_key, "reasoning");
    }
}
