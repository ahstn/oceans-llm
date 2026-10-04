//! Route policies and caller-scoped session continuity for online inference.
use std::collections::BTreeMap;

use gateway_core::{
    GatewayError, ModelRoute, ResponseRouteOrigin, RouteBindingReceipt, RouteSelectionMode,
    RouteSelectionRequest, RoutingCandidate, RoutingRepository, RoutingStrategy,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::{ResolvedGatewayRequest, client_session::extract_routing_session};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingEndpoint {
    ChatCompletions,
    Messages,
    Responses,
    Embeddings,
    Decisions,
}

impl RoutingEndpoint {
    fn supports_affinity(self) -> bool {
        matches!(
            self,
            Self::ChatCompletions | Self::Messages | Self::Responses
        )
    }

    fn key(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Messages => "messages",
            Self::Responses => "responses",
            Self::Embeddings => "embeddings",
            Self::Decisions => "decisions",
        }
    }
}

pub struct RoutingRequest<'a> {
    pub resolved: &'a ResolvedGatewayRequest,
    /// Capability and caller credential checks must run before policy selection.
    /// Preserve the existing weighted planner order for weighted selection.
    pub eligible_routes: &'a [ModelRoute],
    pub extra: &'a BTreeMap<String, Value>,
    pub headers: &'a BTreeMap<String, String>,
    pub harness_key: &'a str,
    pub endpoint: RoutingEndpoint,
    /// Opaque generation IDs for caller-owned provider credentials, keyed by provider.
    pub credential_versions: &'a BTreeMap<String, String>,
    /// Fingerprints of the initialized provider clients on this replica.
    pub provider_identities: &'a BTreeMap<String, String>,
}

pub struct RoutedSelection {
    pub route: ModelRoute,
    pub receipt: Option<RoutingReceipt>,
    pub reused: bool,
}

/// A completion can refresh only the reservation it actually used.
#[derive(Clone)]
pub struct RoutingReceipt {
    binding: Option<RouteBindingReceipt>,
    response_origin: Option<(String, ResponseRouteOrigin)>,
}

impl RoutingReceipt {
    /// Call after a successful guarded response, or a clean stream completion.
    /// Persist response ownership before exposing the completed response to the caller.
    pub async fn complete(
        &self,
        store: &impl RoutingRepository,
        response_id: Option<&str>,
    ) -> Result<(), GatewayError> {
        let now = OffsetDateTime::now_utc();
        self.record_response_origin(store, response_id, now).await?;
        if let Some(binding) = &self.binding
            && store.refresh_route_binding(binding, now).await.is_err()
        {
            // This is a cache preference, so a failed refresh can safely leave
            // the old deadline in place. The caller must still receive billed output.
            tracing::warn!(route_id = %binding.route_id, "failed to refresh session affinity after successful inference");
        }
        Ok(())
    }

    /// Persist a streamed Responses origin before its terminal event is exposed.
    /// Affinity is refreshed separately when the stream finishes successfully.
    pub async fn persist_response_origin(
        &mut self,
        store: &impl RoutingRepository,
        response_id: Option<&str>,
    ) -> Result<(), GatewayError> {
        self.record_response_origin(store, response_id, OffsetDateTime::now_utc())
            .await?;
        if response_id.is_some() {
            self.response_origin = None;
        }
        Ok(())
    }

    async fn record_response_origin(
        &self,
        store: &impl RoutingRepository,
        response_id: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        if let Some((owner, origin)) = &self.response_origin
            && let Some(response_id) = response_id
        {
            validate_response_id(response_id).map_err(|_| {
                GatewayError::Internal("provider returned an invalid response identifier".into())
            })?;
            store
                .record_response_route_origin(owner, &digest(&[response_id]), origin, now)
                .await?;
        }
        Ok(())
    }
}

pub async fn select_route(
    store: &impl RoutingRepository,
    request: RoutingRequest<'_>,
) -> Result<Option<RoutedSelection>, GatewayError> {
    let Some(policy) = &request.resolved.selection.execution_model.routing else {
        return Ok(request
            .eligible_routes
            .first()
            .cloned()
            .map(|route| RoutedSelection {
                route,
                receipt: None,
                reused: false,
            }));
    };
    // Keep stateful resources on their origin. Conversation objects have a separate
    // lifecycle that this first routing implementation does not yet track.
    if request
        .extra
        .get("conversation")
        .is_some_and(|value| !value.is_null())
    {
        return Err(GatewayError::InvalidRequest(
            "conversation resources are not supported by routing pools; send full history or previous_response_id".into(),
        ));
    }
    let owner_key = owner_key(&request);
    let mut candidates = candidates(&request)?;
    if policy.strategy != RoutingStrategy::WeightedRandom {
        candidates.sort_by_key(|candidate| (candidate.priority, candidate.route_id));
    }
    // Parse the session even for a continuation, so conflicting identifiers cannot
    // silently become valid when the caller adds previous_response_id.
    let affinity_key = if policy.affinity.is_some() && request.endpoint.supports_affinity() {
        extract_routing_session(request.extra, request.headers, request.harness_key)?
            .map(|session| digest(&[&owner_key, session.namespace, &session.value]))
    } else {
        None
    };
    let now = OffsetDateTime::now_utc();
    let idle_timeout_seconds = policy
        .affinity
        .as_ref()
        .map_or(3600, |policy| policy.idle_timeout_seconds);
    let selection = if let Some(previous) = previous_response_id(request.extra)? {
        let origin = store
            .get_response_route_origin(&owner_key, &digest(&[previous]), now)
            .await?
            .ok_or_else(unknown_response_origin)?;
        let candidate = candidates.iter().find(|candidate| {
            candidate.route_id == origin.route_id && candidate.fingerprint == origin.fingerprint
        });
        if origin.model_id != request.resolved.selection.execution_model.id || candidate.is_none() {
            return Err(GatewayError::InvalidRequest(
                "the origin route for previous_response_id is no longer eligible; send full history to start a new response".into(),
            ));
        }
        if affinity_key.is_some() {
            // Provider-owned state is authoritative over a soft cache preference.
            // Reserve that origin without advancing the round-robin cursor.
            store
                .select_route(&RouteSelectionRequest {
                    model_id: origin.model_id,
                    affinity_key,
                    candidates: vec![candidate.expect("origin candidate was checked").clone()],
                    mode: RouteSelectionMode::First,
                    now,
                    idle_timeout_seconds,
                })
                .await?
        } else {
            gateway_core::RouteSelection {
                route_id: origin.route_id,
                binding: None,
                reused: true,
            }
        }
    } else if candidates.is_empty() {
        return Ok(None);
    } else if affinity_key.is_some() || policy.strategy == RoutingStrategy::RoundRobin {
        store
            .select_route(&RouteSelectionRequest {
                model_id: request.resolved.selection.execution_model.id,
                affinity_key,
                candidates: candidates.clone(),
                mode: if policy.strategy == RoutingStrategy::RoundRobin {
                    RouteSelectionMode::RoundRobin
                } else {
                    RouteSelectionMode::First
                },
                now,
                idle_timeout_seconds,
            })
            .await?
    } else {
        gateway_core::RouteSelection {
            route_id: candidates[0].route_id,
            binding: None,
            reused: false,
        }
    };
    let route = request
        .eligible_routes
        .iter()
        .find(|route| route.id == selection.route_id)
        .ok_or_else(|| GatewayError::Internal("routing store selected an ineligible route".into()))?
        .clone();
    let response_origin = if request.endpoint == RoutingEndpoint::Responses {
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.route_id == route.id)
            .ok_or_else(|| GatewayError::Internal("selected route has no fingerprint".into()))?;
        Some((
            owner_key,
            ResponseRouteOrigin {
                model_id: request.resolved.selection.execution_model.id,
                route_id: route.id,
                fingerprint: candidate.fingerprint.clone(),
            },
        ))
    } else {
        None
    };
    let receipt =
        (selection.binding.is_some() || response_origin.is_some()).then_some(RoutingReceipt {
            binding: selection.binding,
            response_origin,
        });
    Ok(Some(RoutedSelection {
        route,
        receipt,
        reused: selection.reused,
    }))
}

fn candidates(request: &RoutingRequest<'_>) -> Result<Vec<RoutingCandidate>, GatewayError> {
    request
        .eligible_routes
        .iter()
        .map(|route| {
            let provider_identity = request
                .provider_identities
                .get(&route.provider_key)
                .ok_or_else(|| {
                    GatewayError::Internal("eligible route has no runtime provider identity".into())
                })?;
            let identity = json!({
                "endpoint": request.endpoint.key(),
                "provider": route.provider_key,
                "provider_identity": provider_identity,
                "caller_credential": request.credential_versions.get(&route.provider_key),
                "upstream_model": route.upstream_model,
                "compatibility": route.compatibility,
                "extra_headers": route.extra_headers,
                "extra_body": route.extra_body,
            });
            Ok(RoutingCandidate {
                route_id: route.id,
                priority: route.priority,
                fingerprint: digest(&[&identity.to_string()]),
            })
        })
        .collect()
}

fn owner_key(request: &RoutingRequest<'_>) -> String {
    // The API key is the caller boundary. Aliases intentionally have separate affinity.
    let auth = &request.resolved.auth;
    digest(&[
        "oceans-routing-v1",
        &auth.id.to_string(),
        &auth
            .owner_user_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        &auth
            .owner_team_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        &auth
            .owner_service_account_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        &request.resolved.selection.requested_model.id.to_string(),
    ])
}

fn previous_response_id(extra: &BTreeMap<String, Value>) -> Result<Option<&str>, GatewayError> {
    match extra.get("previous_response_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            validate_response_id(value)?;
            Ok(Some(value))
        }
        Some(_) => Err(GatewayError::InvalidRequest(
            "previous_response_id must be a string".into(),
        )),
    }
}

fn validate_response_id(value: &str) -> Result<(), GatewayError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(GatewayError::InvalidRequest(
            "response identifiers must contain 1 to 256 bytes without control characters".into(),
        ));
    }
    Ok(())
}

fn unknown_response_origin() -> GatewayError {
    GatewayError::InvalidRequest("previous_response_id has no known origin for this API key and model, or its 30-day record expired; send full history to start a new response".into())
}

fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests;
