use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    AuthError, AuthenticatedApiKey, GatewayError, GatewayModel, IdentityRepository,
    ModelAccessMode, ModelRepository, RouteError, UserStatus,
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

        if let Some(service_account_id) = api_key.owner_service_account_id {
            let service_account = self
                .repo
                .get_service_account_by_id(service_account_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if service_account.model_access_mode == ModelAccessMode::Restricted {
                let allowed_for_service_account = self
                    .repo
                    .list_allowed_model_keys_for_service_account(service_account_id)
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                allowed_model_keys =
                    intersect_allowed(allowed_model_keys, allowed_for_service_account);
            }
        }

        if let Some(user_id) = api_key.owner_user_id {
            let user = self
                .repo
                .get_user_by_id(user_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if user.status != UserStatus::Active {
                return Err(AuthError::ApiKeyOwnerInvalid.into());
            }
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
