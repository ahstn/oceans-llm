use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetModelSelector, BudgetScope,
};
use uuid::Uuid;

pub fn applicable_budget_scopes(
    api_key: &AuthenticatedApiKey,
    model_id: Option<Uuid>,
    upstream_model: Option<&str>,
) -> Result<Vec<BudgetScope>, AuthError> {
    match api_key.owner_kind {
        ApiKeyOwnerKind::User => {
            let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            let mut scopes = Vec::with_capacity(2);
            if let Some(model_id) = model_id {
                scopes.push(BudgetScope::UserModel {
                    user_id,
                    selector: BudgetModelSelector::Model { model_id },
                });
            } else if let Some(upstream_model) = upstream_model
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                scopes.push(BudgetScope::UserModel {
                    user_id,
                    selector: BudgetModelSelector::UpstreamModel {
                        upstream_model: upstream_model.to_string(),
                    },
                });
            }
            scopes.push(BudgetScope::User { user_id });
            Ok(scopes)
        }
        ApiKeyOwnerKind::ServiceAccount => {
            let service_account_id = api_key
                .owner_service_account_id
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            Ok(vec![BudgetScope::ServiceAccount { service_account_id }])
        }
    }
}

pub fn usage_ownership_scope_key(api_key: &AuthenticatedApiKey) -> Result<String, AuthError> {
    match api_key.owner_kind {
        ApiKeyOwnerKind::User => {
            let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            Ok(format!("user:{user_id}"))
        }
        ApiKeyOwnerKind::ServiceAccount => {
            let service_account_id = api_key
                .owner_service_account_id
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            Ok(format!("service_account:{service_account_id}"))
        }
    }
}
