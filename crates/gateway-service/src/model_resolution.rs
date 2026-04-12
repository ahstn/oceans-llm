use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gateway_core::{
    AuthenticatedApiKey, GatewayError, GatewayModel, ModelRepository, ProviderConnection,
    RouteError,
};

#[derive(Debug, Clone)]
pub struct ResolvedModelSelection {
    pub requested_model: GatewayModel,
    pub execution_model: GatewayModel,
    pub alias_chain: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedGatewayRequest {
    pub auth: AuthenticatedApiKey,
    pub selection: ResolvedModelSelection,
    pub routes: Vec<gateway_core::ModelRoute>,
    pub provider_connections: HashMap<String, ProviderConnection>,
}

#[derive(Clone)]
pub struct ModelResolver<R> {
    repo: Arc<R>,
}

impl<R> ModelResolver<R>
where
    R: ModelRepository,
{
    const MAX_ALIAS_DEPTH: usize = 8;

    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn canonicalize_requested_model(
        &self,
        requested_model: GatewayModel,
    ) -> Result<ResolvedModelSelection, GatewayError> {
        let requested_model_key = requested_model.model_key.clone();
        let mut current = requested_model.clone();
        let mut seen_keys = BTreeSet::from([requested_model.model_key.clone()]);
        let mut alias_chain = vec![requested_model.model_key.clone()];
        let mut alias_hops = 0usize;

        loop {
            let Some(alias_target_model_key) = current.alias_target_model_key.clone() else {
                return Ok(ResolvedModelSelection {
                    requested_model,
                    execution_model: current,
                    alias_chain,
                });
            };

            if alias_hops >= Self::MAX_ALIAS_DEPTH {
                break;
            }

            let next = self
                .repo
                .get_model_by_key(&alias_target_model_key)
                .await?
                .ok_or_else(|| RouteError::ModelNotFound(requested_model_key.clone()))?;

            if !seen_keys.insert(next.model_key.clone()) {
                return Err(RouteError::Policy(format!(
                    "model alias cycle detected for requested model `{requested_model_key}`"
                ))
                .into());
            }

            alias_chain.push(next.model_key.clone());
            current = next;
            alias_hops += 1;
        }

        Err(RouteError::Policy(format!(
            "model alias depth exceeded for requested model `{requested_model_key}`"
        ))
        .into())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey, GatewayError, GatewayModel, GlobalRole,
        IdentityRepository, ModelAccessMode, ModelRepository, ModelRoute, RouteError, StoreError,
        TeamMembershipRecord, TeamRecord, UserRecord, UserStatus,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::{ModelAccess, ModelResolver};

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        models: HashMap<String, GatewayModel>,
        grants: HashMap<Uuid, Vec<String>>,
    }

    #[async_trait]
    impl ModelRepository for InMemoryRepo {
        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self.models.get(model_key).cloned())
        }

        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(self.models.values().cloned().collect())
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
    impl IdentityRepository for InMemoryRepo {
        async fn get_user_by_id(&self, _user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(Some(UserRecord {
                user_id: Uuid::new_v4(),
                name: "test".to_string(),
                email: "user@example.com".to_string(),
                email_normalized: "user@example.com".to_string(),
                global_role: GlobalRole::User,
                auth_mode: AuthMode::Password,
                status: UserStatus::Active,
                must_change_password: false,
                request_logging_enabled: true,
                model_access_mode: ModelAccessMode::All,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }))
        }

        async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(Some(TeamRecord {
                team_id,
                team_key: "team".to_string(),
                team_name: "Team".to_string(),
                status: "active".to_string(),
                model_access_mode: ModelAccessMode::All,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }))
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

    fn auth(api_key_id: Uuid, team_id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        }
    }

    fn model(model_key: &str, alias_target_model_key: Option<&str>) -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: model_key.to_string(),
            alias_target_model_key: alias_target_model_key.map(ToString::to_string),
            description: None,
            tags: Vec::new(),
            rank: 10,
        }
    }

    #[tokio::test]
    async fn direct_provider_backed_models_resolve_without_alias_hops() {
        let requested = model("fast", None);
        let mut repo = InMemoryRepo::default();
        repo.models
            .insert(requested.model_key.clone(), requested.clone());

        let resolver = ModelResolver::new(Arc::new(repo));
        let resolved = resolver
            .canonicalize_requested_model(requested)
            .await
            .expect("direct model should resolve");

        assert_eq!(resolved.requested_model.model_key, "fast");
        assert_eq!(resolved.execution_model.model_key, "fast");
        assert_eq!(resolved.alias_chain, vec!["fast".to_string()]);
    }

    #[tokio::test]
    async fn alias_chains_resolve_to_the_final_execution_model() {
        let requested = model("fast", Some("fast-v2"));
        let intermediate = model("fast-v2", Some("fast-v3"));
        let target = model("fast-v3", None);

        let mut repo = InMemoryRepo::default();
        repo.models
            .insert(requested.model_key.clone(), requested.clone());
        repo.models
            .insert(intermediate.model_key.clone(), intermediate);
        repo.models.insert(target.model_key.clone(), target.clone());

        let resolver = ModelResolver::new(Arc::new(repo));
        let resolved = resolver
            .canonicalize_requested_model(requested)
            .await
            .expect("alias should resolve");

        assert_eq!(resolved.execution_model.model_key, "fast-v3");
        assert_eq!(
            resolved.alias_chain,
            vec![
                "fast".to_string(),
                "fast-v2".to_string(),
                "fast-v3".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn missing_alias_targets_fail_with_the_requested_model_key() {
        let requested = model("fast", Some("fast-v2"));
        let mut repo = InMemoryRepo::default();
        repo.models
            .insert(requested.model_key.clone(), requested.clone());

        let resolver = ModelResolver::new(Arc::new(repo));
        let error = resolver
            .canonicalize_requested_model(requested)
            .await
            .expect_err("missing target should fail");

        assert!(matches!(
            error,
            GatewayError::Route(RouteError::ModelNotFound(model_key)) if model_key == "fast"
        ));
    }

    #[tokio::test]
    async fn runtime_alias_cycles_are_rejected() {
        let fast = model("fast", Some("fast-v2"));
        let fast_v2 = model("fast-v2", Some("fast"));

        let mut repo = InMemoryRepo::default();
        repo.models.insert(fast.model_key.clone(), fast.clone());
        repo.models.insert(fast_v2.model_key.clone(), fast_v2);

        let resolver = ModelResolver::new(Arc::new(repo));
        let error = resolver
            .canonicalize_requested_model(fast)
            .await
            .expect_err("cycle should fail");

        assert!(matches!(error, GatewayError::Route(RouteError::Policy(_))));
    }

    #[tokio::test]
    async fn alias_depth_limit_is_enforced() {
        let mut repo = InMemoryRepo::default();
        let requested = model("fast", Some("fast-v1"));
        repo.models
            .insert(requested.model_key.clone(), requested.clone());
        for index in 1..=9 {
            let model_key = format!("fast-v{index}");
            let alias_target_model_key = (index < 9).then(|| format!("fast-v{}", index + 1));
            repo.models.insert(
                model_key.clone(),
                GatewayModel {
                    id: Uuid::new_v4(),
                    model_key,
                    alias_target_model_key,
                    description: None,
                    tags: Vec::new(),
                    rank: 10,
                },
            );
        }

        let resolver = ModelResolver::new(Arc::new(repo));
        let error = resolver
            .canonicalize_requested_model(requested)
            .await
            .expect_err("excessive alias depth should fail");

        assert!(matches!(error, GatewayError::Route(RouteError::Policy(_))));
    }

    #[tokio::test]
    async fn alias_depth_limit_allows_exact_boundary_hops() {
        let mut repo = InMemoryRepo::default();
        let requested = model("fast", Some("fast-v1"));
        repo.models
            .insert(requested.model_key.clone(), requested.clone());
        for index in 1..=8 {
            let model_key = format!("fast-v{index}");
            let alias_target_model_key = (index < 8).then(|| format!("fast-v{}", index + 1));
            repo.models.insert(
                model_key.clone(),
                GatewayModel {
                    id: Uuid::new_v4(),
                    model_key,
                    alias_target_model_key,
                    description: None,
                    tags: Vec::new(),
                    rank: 10,
                },
            );
        }

        let resolver = ModelResolver::new(Arc::new(repo));
        let resolved = resolver
            .canonicalize_requested_model(requested)
            .await
            .expect("boundary alias depth should resolve");

        assert_eq!(resolved.execution_model.model_key, "fast-v8");
        assert_eq!(resolved.alias_chain.len(), 9);
    }

    #[tokio::test]
    async fn tag_resolution_is_followed_by_alias_canonicalization() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let requested = GatewayModel {
            id: Uuid::new_v4(),
            model_key: "fast".to_string(),
            alias_target_model_key: Some("fast-v2".to_string()),
            description: None,
            tags: vec!["fast".to_string(), "cheap".to_string()],
            rank: 10,
        };
        let target = GatewayModel {
            id: Uuid::new_v4(),
            model_key: "fast-v2".to_string(),
            alias_target_model_key: None,
            description: None,
            tags: vec!["fast".to_string(), "cheap".to_string()],
            rank: 5,
        };
        let mut repo = InMemoryRepo::default();
        repo.models
            .insert(requested.model_key.clone(), requested.clone());
        repo.models.insert(target.model_key.clone(), target.clone());
        repo.grants.insert(api_key_id, vec!["fast".to_string()]);

        let repo = Arc::new(repo);
        let model_access = ModelAccess::new(repo.clone());
        let resolver = ModelResolver::new(repo);

        let requested_model = model_access
            .resolve_requested_model(&auth(api_key_id, team_id), "tag:fast,cheap")
            .await
            .expect("tag expression should resolve");
        let resolved = resolver
            .canonicalize_requested_model(requested_model)
            .await
            .expect("alias should canonicalize");

        assert_eq!(resolved.requested_model.model_key, "fast");
        assert_eq!(resolved.execution_model.model_key, "fast-v2");
    }
}
