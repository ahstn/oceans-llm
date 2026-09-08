use super::*;
use gateway_core::{Money4, ProviderCapabilities, RoutePricingOverride};
use serde_json::json;
use uuid::Uuid;

fn route() -> ModelRoute {
    ModelRoute {
        id: Uuid::new_v4(),
        model_id: Uuid::new_v4(),
        provider_key: "test".into(),
        upstream_model: "gpt-5".into(),
        priority: 0,
        weight: 1.0,
        enabled: true,
        context_window_tokens: Some(1000),
        pricing_override: None,
        extra_headers: Default::default(),
        extra_body: Default::default(),
        capabilities: ProviderCapabilities::all_enabled(),
        compatibility: Default::default(),
    }
}

fn provider() -> ProviderConnection {
    ProviderConnection {
        provider_key: "test".into(),
        provider_type: "openai_compat".into(),
        config: json!({"pricing_provider_id":"openai"}),
        secrets: None,
    }
}

#[test]
fn multiple_routes_keep_unknown_limits_and_intersect_capabilities() {
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    let known = route_metadata(&route(), Some(&provider()), &snapshot);
    assert_eq!(known.limits.context, Some(1000));
    assert!(known.limits.output.is_some_and(|value| value <= 1000));
    let mut unknown_route = route();
    unknown_route.context_window_tokens = None;
    unknown_route.upstream_model = "unlisted".into();
    unknown_route.capabilities.tools = false;
    let unknown = route_metadata(&unknown_route, Some(&provider()), &snapshot);
    let combined = summarize("public-model".into(), vec![known, unknown]);
    assert_eq!(combined.enabled_route_count, 2);
    assert_eq!(combined.limits.context, None);
    assert_eq!(combined.capabilities.tools, Some(false));
    assert_eq!(combined.capabilities.reasoning, None);
    let empty = summarize("unroutable".into(), vec![]);
    assert_eq!(empty.capabilities.tools, None);
}

#[test]
fn billing_modifiers_suppress_catalog_prices_but_preserve_explicit_overrides() {
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    let mut route = route();
    route
        .extra_body
        .insert("service_tier".into(), json!("priority"));
    let unsupported = route_metadata(&route, Some(&provider()), &snapshot);
    assert!(unsupported.pricing.is_none());
    route.pricing_override = Some(RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(0),
        output_cost_per_million_tokens: Money4::from_scaled(12345),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
    });
    let configured = route_metadata(&route, Some(&provider()), &snapshot);
    assert_eq!(configured.pricing_source, Some("configured_override"));
    assert_eq!(configured.pricing.unwrap().input.as_deref(), Some("0.0000"));
}

#[test]
fn supplement_requires_explicit_provider_identity_and_never_mutates_billing_snapshot() {
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    let before = serde_json::to_value(&snapshot.document).unwrap();
    let mut unknown_provider = provider();
    unknown_provider.config = json!({"base_url":"https://api.openai.com/v1"});
    let unknown = route_metadata(&route(), Some(&unknown_provider), &snapshot);
    assert!(unknown.catalog_metadata.is_none());
    assert!(unknown.pricing.is_none());
    let known = route_metadata(&route(), Some(&provider()), &snapshot);
    assert!(known.catalog_metadata.is_some());
    assert_eq!(before, serde_json::to_value(&snapshot.document).unwrap());
}

struct BatchRepo {
    models: Vec<GatewayModel>,
    routes: Vec<ModelRoute>,
}

#[async_trait::async_trait]
impl ModelRepository for BatchRepo {
    async fn list_models(&self) -> Result<Vec<GatewayModel>, gateway_core::StoreError> {
        Ok(self.models.clone())
    }
    async fn get_model_by_key(
        &self,
        _: &str,
    ) -> Result<Option<GatewayModel>, gateway_core::StoreError> {
        panic!("must batch aliases")
    }
    async fn list_models_for_api_key(
        &self,
        _: Uuid,
    ) -> Result<Vec<GatewayModel>, gateway_core::StoreError> {
        panic!("access is resolved before metadata")
    }
    async fn list_model_allowlists_for_models(
        &self,
        _: &[Uuid],
    ) -> Result<HashMap<Uuid, gateway_core::ModelAllowlistPolicy>, gateway_core::StoreError> {
        panic!("access is resolved before metadata")
    }
    async fn list_routes_for_model(
        &self,
        _: Uuid,
    ) -> Result<Vec<ModelRoute>, gateway_core::StoreError> {
        panic!("must batch routes")
    }
    async fn list_routes_for_models(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ModelRoute>>, gateway_core::StoreError> {
        assert_eq!(ids, &[self.models[0].id]);
        Ok(HashMap::from([(ids[0], self.routes.clone())]))
    }
}

#[async_trait::async_trait]
impl ProviderRepository for BatchRepo {
    async fn get_provider_by_key(
        &self,
        _: &str,
    ) -> Result<Option<ProviderConnection>, gateway_core::StoreError> {
        panic!("must batch providers")
    }
    async fn list_providers_by_keys(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, ProviderConnection>, gateway_core::StoreError> {
        assert_eq!(keys, &["test"]);
        Ok(HashMap::from([("test".into(), provider())]))
    }
}

#[tokio::test]
async fn aliases_use_batched_target_routes_without_exposing_ungranted_model_ids() {
    let target = GatewayModel {
        id: Uuid::new_v4(),
        model_key: "private-target".into(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: vec![],
        rank: 0,
    };
    let alias = GatewayModel {
        id: Uuid::new_v4(),
        model_key: "public-alias".into(),
        alias_target_model_key: Some(target.model_key.clone()),
        ..target.clone()
    };
    let mut enabled = route();
    enabled.model_id = target.id;
    let mut disabled = enabled.clone();
    disabled.enabled = false;
    disabled.provider_key = "disabled-provider".into();
    let repo = BatchRepo {
        models: vec![target, alias.clone()],
        routes: vec![enabled, disabled],
    };
    let response = list_metadata(
        &repo,
        vec![alias],
        &crate::pricing_catalog::load_vendored_fallback_snapshot(),
    )
    .await
    .unwrap();
    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].id, "public-alias");
    assert_eq!(response.data[0].enabled_route_count, 1);
    assert!(
        !serde_json::to_string(&response)
            .unwrap()
            .contains("private-target")
    );
}

#[test]
fn broken_and_cyclic_aliases_have_no_execution_metadata() {
    let model = GatewayModel {
        id: Uuid::new_v4(),
        model_key: "alias".into(),
        alias_target_model_key: Some("missing".into()),
        max_reasoning_effort: None,
        description: None,
        tags: vec![],
        rank: 0,
    };
    assert!(execution_model_from_snapshot(&HashMap::new(), &model).is_none());
    let cycle = GatewayModel {
        alias_target_model_key: Some("alias".into()),
        ..model
    };
    assert!(execution_model_from_snapshot(&HashMap::from([("alias", &cycle)]), &cycle).is_none());
}
