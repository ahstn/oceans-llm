use std::sync::Arc;

use gateway_core::{
    AuthError, AuthenticatedApiKey, GatewayError, GatewayModel, ModelRepository, RouteError,
};
use itertools::Itertools;

#[derive(Clone)]
pub struct ModelAccess<R> {
    repo: Arc<R>,
}

impl<R> ModelAccess<R>
where
    R: ModelRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_models_for_api_key(
        &self,
        api_key_id: uuid::Uuid,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        self.repo
            .list_models_for_api_key(api_key_id)
            .await
            .map_err(Into::into)
    }

    pub async fn resolve_requested_model(
        &self,
        api_key: &AuthenticatedApiKey,
        requested_model: &str,
    ) -> Result<GatewayModel, GatewayError> {
        if let Some(tag_expression) = requested_model.strip_prefix("tag:") {
            return self
                .resolve_tag_expression(api_key.id, tag_expression)
                .await;
        }

        let model = self
            .repo
            .get_model_by_key(requested_model)
            .await?
            .ok_or_else(|| RouteError::ModelNotFound(requested_model.to_string()))?;

        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
        let has_grant = granted_models
            .iter()
            .any(|granted| granted.model_key == requested_model);

        if !has_grant {
            return Err(AuthError::ModelNotGranted(requested_model.to_string()).into());
        }

        Ok(model)
    }

    async fn resolve_tag_expression(
        &self,
        api_key_id: uuid::Uuid,
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

        let granted_models = self.repo.list_models_for_api_key(api_key_id).await?;

        granted_models
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
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use async_trait::async_trait;
    use gateway_core::{
        AuthenticatedApiKey, GatewayError, GatewayModel, ModelRepository, ModelRoute, StoreError,
    };
    use uuid::Uuid;

    use super::ModelAccess;

    #[derive(Clone, Default)]
    struct InMemoryModelRepo {
        models: HashMap<String, GatewayModel>,
        grants: HashMap<Uuid, Vec<String>>,
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

    #[tokio::test]
    async fn resolves_tag_expression_by_rank_then_model_key() {
        let api_key_id = Uuid::new_v4();
        let fast = GatewayModel {
            id: Uuid::new_v4(),
            model_key: "fast".to_string(),
            description: None,
            tags: vec!["fast".to_string(), "cheap".to_string()],
            rank: 20,
        };
        let fast_alt = GatewayModel {
            id: Uuid::new_v4(),
            model_key: "fast-alt".to_string(),
            description: None,
            tags: vec!["fast".to_string(), "cheap".to_string()],
            rank: 10,
        };

        let mut repo = InMemoryModelRepo::default();
        repo.models.insert(fast.model_key.clone(), fast);
        repo.models.insert(fast_alt.model_key.clone(), fast_alt);
        repo.grants
            .insert(api_key_id, vec!["fast".to_string(), "fast-alt".to_string()]);

        let access = ModelAccess::new(Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
        };

        let resolved = access
            .resolve_requested_model(&auth, "tag:fast,cheap")
            .await
            .expect("tag resolution should succeed");

        assert_eq!(resolved.model_key, "fast-alt");
    }

    #[tokio::test]
    async fn rejects_ungranted_model() {
        let api_key_id = Uuid::new_v4();
        let model = GatewayModel {
            id: Uuid::new_v4(),
            model_key: "reasoning".to_string(),
            description: None,
            tags: vec!["reasoning".to_string()],
            rank: 1,
        };

        let mut repo = InMemoryModelRepo::default();
        repo.models.insert(model.model_key.clone(), model);
        repo.grants.insert(api_key_id, vec![]);

        let access = ModelAccess::new(Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
        };

        let error = access
            .resolve_requested_model(&auth, "reasoning")
            .await
            .expect_err("must reject ungranted model");

        assert!(matches!(error, GatewayError::Auth(_)));
    }
}
