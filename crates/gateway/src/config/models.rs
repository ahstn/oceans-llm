use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAllowlistConfig {
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub teams: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    #[serde(default)]
    pub alias_of: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_model_rank")]
    pub rank: i32,
    #[serde(default)]
    pub routes: Vec<ModelRouteConfig>,
    pub allowlist: Option<ModelAllowlistConfig>,
}

pub(super) fn validate_models(
    models: &[ModelConfig],
    model_by_id: &BTreeMap<&str, &ModelConfig>,
    provider_by_id: &BTreeMap<String, &ProviderConfig>,
) -> anyhow::Result<()> {
    for model in models {
        let has_alias = model.alias_of.is_some();
        let has_routes = !model.routes.is_empty();

        match (has_alias, has_routes) {
            (true, true) => bail!(
                "model `{}` cannot define both alias_of and routes",
                model.id
            ),
            (false, false) => bail!(
                "model `{}` must define either alias_of or at least one route",
                model.id
            ),
            _ => {}
        }

        if let Some(alias_target) = model.alias_of.as_deref() {
            if alias_target == model.id {
                bail!("model `{}` cannot alias itself", model.id);
            }
            if !model_by_id.contains_key(alias_target) {
                bail!(
                    "model `{}` aliases unknown model `{alias_target}`",
                    model.id
                );
            }
        }

        // Validate here so config loading fails even if callers never request seed models.
        if let Some(allowlist) = &model.allowlist {
            normalize_model_allowlist(&model.id, allowlist)?;
        }

        for route in &model.routes {
            if let Some(context_window_tokens) = route.context_window_tokens
                && context_window_tokens <= 0
            {
                bail!(
                    "model `{}` route `{}` context_window_tokens must be positive",
                    model.id,
                    route.upstream_model
                );
            }
            if let Some(pricing_override) = &route.pricing_override {
                pricing_override.resolve(&format!(
                    "model `{}` route `{}` pricing_override",
                    model.id, route.upstream_model
                ))?;
            }

            let provider = provider_by_id.get(route.provider.as_str()).copied();

            if provider.is_some_and(|provider| matches!(provider, ProviderConfig::GcpVertex(_))) {
                routes::validate_vertex_upstream_model_format(&route.upstream_model)?;
            }
            if let Some(openrouter) = &route.compatibility.openrouter {
                routes::validate_openrouter_route_compatibility(
                    &model.id, route, provider, openrouter,
                )?;
            }
            if route.compatibility.github_copilot.is_some()
                && !provider
                    .is_some_and(|provider| matches!(provider, ProviderConfig::GitHubCopilot(_)))
            {
                bail!(
                    "model `{}` route for provider `{}` uses compatibility.github_copilot but requires a github_copilot provider",
                    model.id,
                    route.provider
                );
            }
            if let Some(ProviderConfig::AwsBedrock(provider)) = provider {
                routes::validate_aws_bedrock_route_compatibility(&model.id, route, provider)?;
            }
        }
    }

    for model in models {
        let mut seen = std::collections::BTreeSet::new();
        let mut current = model;

        while let Some(alias_target) = current.alias_of.as_deref() {
            if !seen.insert(current.id.as_str()) {
                bail!("model alias cycle detected starting at `{}`", model.id);
            }

            current = model_by_id.get(alias_target).copied().ok_or_else(|| {
                anyhow::anyhow!(
                    "model `{}` aliases unknown model `{alias_target}`",
                    model.id
                )
            })?;
        }
    }

    Ok(())
}
