//! Provider eligibility and route selection for inference requests.
use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use axum::http::HeaderMap;

use gateway_core::{
    AuthenticatedApiKey, CoreRequestRequirements, GatewayError, ModelRoute, ProviderCapabilities,
    ProviderClient, ProviderUserCredentialRepository, vertex_route_capabilities_for_upstream_model,
};

use gateway_service::{
    ResolvedGatewayRequest,
    request_logging::classify_agent_harness,
    routing::{RoutingEndpoint, RoutingReceipt, RoutingRequest, select_route},
};
use serde_json::Value;

use super::AppState;

pub(super) struct SelectedProviderRoute {
    pub route: gateway_core::ModelRoute,
    pub provider: Arc<dyn ProviderClient>,
    pub receipt: Option<RoutingReceipt>,
    pub expected_provider_credential_id: Option<uuid::Uuid>,
}

pub(super) async fn select_provider_route(
    state: &AppState,
    resolved: &ResolvedGatewayRequest,
    requirements: CoreRequestRequirements,
    headers: &HeaderMap,
    extra: &BTreeMap<String, Value>,
    request_headers: &BTreeMap<String, String>,
    endpoint: RoutingEndpoint,
) -> Result<(usize, Option<SelectedProviderRoute>), GatewayError> {
    let policy = resolved.selection.execution_model.routing.as_ref();
    if policy.is_some_and(|policy| policy.affinity.is_some())
        && matches!(
            endpoint,
            RoutingEndpoint::ChatCompletions
                | RoutingEndpoint::Messages
                | RoutingEndpoint::Responses
        )
    {
        validate_session_headers(headers)?;
    }
    let eligible = eligible_routes(&state.providers, &resolved.routes, requirements);
    let (eligible, credential_versions) = if policy.is_some() {
        filter_credential_routes(state, &resolved.auth, eligible).await?
    } else {
        (eligible, BTreeMap::new())
    };
    let provider_identities = if policy.is_some() {
        eligible
            .iter()
            .filter_map(|route| {
                state
                    .providers
                    .routing_identity(&route.provider_key)
                    .map(|identity| (route.provider_key.clone(), identity.to_string()))
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    let eligible_route_count = eligible.len();
    let harness = classify_agent_harness(request_headers.get("user-agent").map(String::as_str));
    let selected = select_route(
        state.store.as_ref(),
        RoutingRequest {
            resolved,
            eligible_routes: &eligible,
            extra,
            headers: request_headers,
            harness_key: harness.key,
            endpoint,
            credential_versions: &credential_versions,
            provider_identities: &provider_identities,
        },
    )
    .await?;
    let selected = selected
        .map(|selected| {
            let expected_provider_credential_id = credential_versions
                .get(&selected.route.provider_key)
                .map(|version| uuid::Uuid::parse_str(version))
                .transpose()
                .map_err(|error| {
                    GatewayError::Internal(format!(
                        "invalid provider credential generation: {error}"
                    ))
                })?;
            tracing::debug!(route_id = %selected.route.id, affinity_reused = selected.reused,
            "provider route selected");
            // Eligibility was checked against this immutable request's provider registry.
            let provider = state
                .providers
                .get(&selected.route.provider_key)
                .expect("selected route has a registered provider");
            Ok::<_, GatewayError>(SelectedProviderRoute {
                route: selected.route,
                provider,
                receipt: selected.receipt,
                expected_provider_credential_id,
            })
        })
        .transpose()?;
    Ok((eligible_route_count, selected))
}

/// Checks each per-user provider once, including missing credentials, within this request.
async fn filter_credential_routes(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    routes: Vec<ModelRoute>,
) -> Result<(Vec<ModelRoute>, BTreeMap<String, String>), GatewayError> {
    let mut permitted = Vec::with_capacity(routes.len());
    let mut credential_cache = BTreeMap::new();
    let mut credential_versions = BTreeMap::new();
    for route in routes {
        if state
            .copilot_user_provider_keys
            .contains(&route.provider_key)
        {
            let Some(user_id) = auth.owner_user_id else {
                continue;
            };
            let version = match credential_cache.entry(route.provider_key.clone()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let credential = state
                        .store
                        .get_provider_user_credential(&route.provider_key, user_id)
                        .await?;
                    entry.insert(credential.map(|credential| credential.credential_id.to_string()))
                }
            };
            let Some(version) = version else {
                continue;
            };
            credential_versions.insert(route.provider_key.clone(), version.clone());
        }
        permitted.push(route);
    }
    Ok((permitted, credential_versions))
}

fn validate_session_headers(headers: &HeaderMap) -> Result<(), GatewayError> {
    for name in [
        "x-oceans-session-id",
        "session-id",
        "session_id",
        "x-claude-code-session-id",
        "x-session-id",
        "x-session-affinity",
        "x-opencode-session",
        "x-client-request-id",
        "x-codex-turn-metadata",
    ] {
        let mut values = headers.get_all(name).iter();
        let Some(value) = values.next() else {
            continue;
        };
        if values.next().is_some() {
            return Err(GatewayError::InvalidRequest(format!(
                "header `{name}` may only be sent once"
            )));
        }
        value.to_str().map_err(|_| {
            GatewayError::InvalidRequest(format!("header `{name}` must be valid UTF-8 text"))
        })?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn select_first_eligible_route(
    providers: &gateway_core::ProviderRegistry,
    routes: &[gateway_core::ModelRoute],
    requirements: CoreRequestRequirements,
) -> (usize, Option<SelectedProviderRoute>) {
    let eligible = eligible_routes(providers, routes, requirements);
    let selected = eligible.first().map(|route| SelectedProviderRoute {
        route: route.clone(),
        provider: providers
            .get(&route.provider_key)
            .expect("eligible provider"),
        receipt: None,
        expected_provider_credential_id: None,
    });
    (eligible.len(), selected)
}

fn eligible_routes(
    providers: &gateway_core::ProviderRegistry,
    routes: &[gateway_core::ModelRoute],
    requirements: CoreRequestRequirements,
) -> Vec<gateway_core::ModelRoute> {
    routes
        .iter()
        .filter(|route| {
            let Some(provider) = providers.get(&route.provider_key) else {
                return false;
            };
            let capabilities =
                route_capabilities_for_request(provider.as_ref(), route, requirements)
                    .intersect(route.capabilities);
            supports_requirements(capabilities, requirements)
        })
        .cloned()
        .collect()
}

pub(super) fn route_capabilities_for_request(
    provider: &dyn ProviderClient,
    route: &gateway_core::ModelRoute,
    requirements: CoreRequestRequirements,
) -> ProviderCapabilities {
    let mut capabilities = route_effective_provider_capabilities(provider, route);
    if provider.provider_type() == "github_copilot"
        && requirements.chat_completions
        && route
            .compatibility
            .github_copilot
            .as_ref()
            .is_some_and(|compatibility| {
                compatibility.chat_api
                    == Some(gateway_core::GitHubCopilotChatApi::AnthropicMessages)
            })
    {
        capabilities.json_schema = false;
    }
    capabilities
}

pub(super) fn route_effective_provider_capabilities(
    provider: &dyn ProviderClient,
    route: &gateway_core::ModelRoute,
) -> ProviderCapabilities {
    if provider.provider_type() == "gcp_vertex" {
        return vertex_route_capabilities_for_upstream_model(Some(&route.upstream_model));
    }
    if provider.provider_type() == "github_copilot" {
        return gateway_core::github_copilot_route_capabilities(
            route.compatibility.github_copilot.as_ref(),
        );
    }
    if provider.provider_type() == "openai_compat"
        && route
            .compatibility
            .openrouter
            .as_ref()
            .is_some_and(|openrouter| openrouter.api.is_decisions())
    {
        let mut capabilities = provider.capabilities();
        capabilities.decisions = true;
        return capabilities;
    }

    provider.capabilities()
}

fn supports_requirements(
    capabilities: ProviderCapabilities,
    requirements: CoreRequestRequirements,
) -> bool {
    (!requirements.chat_completions || capabilities.chat_completions)
        && (!requirements.responses || capabilities.responses)
        && (!requirements.stream || capabilities.stream)
        && (!requirements.embeddings || capabilities.embeddings)
        && (!requirements.decisions || capabilities.decisions)
        && (!requirements.tools || capabilities.tools)
        && (!requirements.vision || capabilities.vision)
        && (!requirements.json_schema || capabilities.json_schema)
        && (!requirements.developer_role || capabilities.developer_role)
}

pub(super) fn no_compatible_route_error(requirements: CoreRequestRequirements) -> GatewayError {
    let required = requirements.required_capability_names();
    let required = if required.is_empty() {
        "none".to_string()
    } else {
        required.join(", ")
    };
    GatewayError::InvalidRequest(format!(
        "no configured route supports requested capabilities ({required})"
    ))
}
