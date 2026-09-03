use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetModelSelector, BudgetScope,
};
use uuid::Uuid;

/// Budget scopes to check for one request, in evaluation order.
///
/// Human traffic checks the matching user-model budget first, then the user
/// budget. The upstream-model selector is only used when no gateway model id is
/// available. Service-account traffic checks only the service-account budget.
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

#[cfg(test)]
mod tests {
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetModelSelector,
        BudgetScope,
    };
    use uuid::Uuid;

    use super::{applicable_budget_scopes, usage_ownership_scope_key};

    fn api_key(
        owner_kind: ApiKeyOwnerKind,
        user: Option<Uuid>,
        sa: Option<Uuid>,
    ) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "k".to_string(),
            name: "k".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind,
            owner_user_id: user,
            owner_team_id: None,
            owner_service_account_id: sa,
        }
    }

    #[test]
    fn user_with_model_id_checks_model_scope_then_user_scope() {
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let key = api_key(ApiKeyOwnerKind::User, Some(user_id), None);

        let scopes = applicable_budget_scopes(&key, Some(model_id), Some("ignored-when-id-known"))
            .expect("scopes");

        assert_eq!(
            scopes,
            vec![
                BudgetScope::UserModel {
                    user_id,
                    selector: BudgetModelSelector::Model { model_id },
                },
                BudgetScope::User { user_id },
            ]
        );
    }

    #[test]
    fn user_without_model_id_falls_back_to_trimmed_upstream_model() {
        let user_id = Uuid::new_v4();
        let key = api_key(ApiKeyOwnerKind::User, Some(user_id), None);

        let scopes = applicable_budget_scopes(&key, None, Some("  gpt-5  ")).expect("scopes");

        assert_eq!(
            scopes[0],
            BudgetScope::UserModel {
                user_id,
                selector: BudgetModelSelector::UpstreamModel {
                    upstream_model: "gpt-5".to_string(),
                },
            }
        );
        assert_eq!(scopes[1], BudgetScope::User { user_id });
    }

    #[test]
    fn user_without_any_model_information_checks_only_user_scope() {
        let user_id = Uuid::new_v4();
        let key = api_key(ApiKeyOwnerKind::User, Some(user_id), None);

        assert_eq!(
            applicable_budget_scopes(&key, None, Some("   ")).expect("scopes"),
            vec![BudgetScope::User { user_id }]
        );
        assert_eq!(
            applicable_budget_scopes(&key, None, None).expect("scopes"),
            vec![BudgetScope::User { user_id }]
        );
    }

    #[test]
    fn service_account_checks_only_its_own_scope() {
        let service_account_id = Uuid::new_v4();
        let key = api_key(
            ApiKeyOwnerKind::ServiceAccount,
            None,
            Some(service_account_id),
        );

        let scopes =
            applicable_budget_scopes(&key, Some(Uuid::new_v4()), Some("gpt-5")).expect("scopes");

        assert_eq!(
            scopes,
            vec![BudgetScope::ServiceAccount { service_account_id }]
        );
        assert_eq!(
            usage_ownership_scope_key(&key).expect("key"),
            format!("service_account:{service_account_id}")
        );
    }

    #[test]
    fn missing_owner_id_is_an_owner_invariant_error() {
        let user_key = api_key(ApiKeyOwnerKind::User, None, None);
        let sa_key = api_key(ApiKeyOwnerKind::ServiceAccount, None, None);

        assert!(matches!(
            applicable_budget_scopes(&user_key, None, None),
            Err(AuthError::ApiKeyOwnerInvalid)
        ));
        assert!(matches!(
            usage_ownership_scope_key(&sa_key),
            Err(AuthError::ApiKeyOwnerInvalid)
        ));
    }
}
