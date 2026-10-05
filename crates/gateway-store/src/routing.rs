use gateway_core::{
    RouteBindingReceipt, RouteSelection, RouteSelectionMode, RouteSelectionRequest,
    RoutingCandidate, StoreError,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub(crate) const RESPONSE_ORIGIN_RETENTION_SECONDS: u32 = 30 * 24 * 60 * 60;

pub(crate) struct StoredRouteBinding {
    pub route_id: Uuid,
    pub fingerprint: String,
    pub token: Uuid,
    pub expires_at: i64,
}

pub(crate) fn cooldown_keys(request: &RouteSelectionRequest) -> Vec<&str> {
    let mut keys = request
        .candidates
        .iter()
        .filter_map(|candidate| candidate.cooldown_key.as_deref())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub(crate) fn binding_expiry(now: OffsetDateTime, timeout_seconds: u32) -> Result<i64, StoreError> {
    if timeout_seconds == 0 {
        return Err(StoreError::Serialization(
            "routing idle timeout must be positive".to_string(),
        ));
    }
    now.checked_add(Duration::seconds(i64::from(timeout_seconds)))
        .map(OffsetDateTime::unix_timestamp)
        .ok_or_else(|| {
            StoreError::Serialization("routing expiry exceeds supported time".to_string())
        })
}

pub(crate) fn reuse_binding(
    request: &RouteSelectionRequest,
    binding: &StoredRouteBinding,
) -> Option<RouteSelection> {
    let affinity_key = request.affinity_key.as_ref()?;
    if binding.expires_at <= request.now.unix_timestamp()
        || !request.candidates.iter().any(|candidate| {
            candidate.route_id == binding.route_id && candidate.fingerprint == binding.fingerprint
        })
    {
        return None;
    }
    Some(RouteSelection {
        route_id: binding.route_id,
        binding: Some(RouteBindingReceipt {
            model_id: request.model_id,
            affinity_key: affinity_key.clone(),
            token: binding.token,
            route_id: binding.route_id,
            idle_timeout_seconds: request.idle_timeout_seconds,
        }),
        reused: true,
    })
}

pub(crate) fn choose_route(
    request: &RouteSelectionRequest,
    cursor: i64,
) -> Result<(&RoutingCandidate, i64), StoreError> {
    let priority = request
        .candidates
        .iter()
        .map(|route| route.priority)
        .min()
        .ok_or_else(|| StoreError::NotFound("no eligible model route".to_string()))?;
    let mut tier = request
        .candidates
        .iter()
        .filter(|route| route.priority == priority);
    if request.mode == RouteSelectionMode::First {
        return Ok((tier.next().expect("minimum priority exists"), cursor));
    }
    let mut tier = tier.collect::<Vec<_>>();
    tier.sort_unstable_by_key(|candidate| candidate.route_id);
    let count =
        i64::try_from(tier.len()).map_err(|error| StoreError::Serialization(error.to_string()))?;
    let index = cursor.rem_euclid(count);
    Ok((tier[index as usize], (index + 1) % count))
}

/// Each eligible priority tier has its own cursor, shared across replicas.
/// Input order and fingerprints do not change that cursor's route membership.
pub(crate) fn routing_pool_key(request: &RouteSelectionRequest) -> Result<Uuid, StoreError> {
    let priority = request
        .candidates
        .iter()
        .map(|route| route.priority)
        .min()
        .ok_or_else(|| StoreError::NotFound("no eligible model route".to_string()))?;
    let mut route_ids = request
        .candidates
        .iter()
        .filter(|route| route.priority == priority)
        .map(|route| route.route_id)
        .collect::<Vec<_>>();
    route_ids.sort_unstable();
    let identity = route_ids
        .iter()
        .flat_map(|id| id.as_bytes().iter().copied())
        .collect::<Vec<_>>();
    Ok(Uuid::new_v5(&request.model_id, &identity))
}

pub(crate) fn new_selection(
    request: &RouteSelectionRequest,
    candidate: &RoutingCandidate,
) -> RouteSelection {
    RouteSelection {
        route_id: candidate.route_id,
        binding: request
            .affinity_key
            .as_ref()
            .map(|affinity_key| RouteBindingReceipt {
                model_id: request.model_id,
                affinity_key: affinity_key.clone(),
                token: Uuid::new_v4(),
                route_id: candidate.route_id,
                idle_timeout_seconds: request.idle_timeout_seconds,
            }),
        reused: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RouteSelectionRequest {
        RouteSelectionRequest {
            model_id: Uuid::from_u128(9),
            affinity_key: None,
            candidates: [3, 1, 2]
                .into_iter()
                .map(|id| RoutingCandidate {
                    route_id: Uuid::from_u128(id),
                    fingerprint: id.to_string(),
                    priority: 10,
                    cooldown_key: None,
                })
                .collect(),
            mode: RouteSelectionMode::RoundRobin,
            now: OffsetDateTime::UNIX_EPOCH,
            idle_timeout_seconds: 3_600,
        }
    }

    #[test]
    fn cursor_identity_tracks_model_and_eligible_tier_membership() {
        let mut request = request();
        let original = routing_pool_key(&request).unwrap();
        request.candidates.reverse();
        request.candidates[0].fingerprint = "changed-account".to_string();
        assert_eq!(routing_pool_key(&request).unwrap(), original);
        request.candidates.push(RoutingCandidate {
            route_id: Uuid::from_u128(4),
            fingerprint: "fallback".to_string(),
            priority: 20,
            cooldown_key: None,
        });
        assert_eq!(routing_pool_key(&request).unwrap(), original);
        request.model_id = Uuid::from_u128(10);
        assert_ne!(routing_pool_key(&request).unwrap(), original);
        request.model_id = Uuid::from_u128(9);
        request.candidates[0].priority = 20;
        assert_ne!(routing_pool_key(&request).unwrap(), original);
    }

    #[test]
    fn round_robin_orders_the_tier_but_first_keeps_service_order() {
        let mut request = request();
        let (route, next) = choose_route(&request, 0).unwrap();
        assert_eq!(route.route_id, Uuid::from_u128(1));
        assert_eq!(next, 1);
        assert_eq!(choose_route(&request, 2).unwrap().1, 0);
        request.mode = RouteSelectionMode::First;
        assert_eq!(
            choose_route(&request, 0).unwrap().0.route_id,
            Uuid::from_u128(3)
        );
        request.candidates.clear();
        assert!(choose_route(&request, 0).is_err());
        assert!(routing_pool_key(&request).is_err());
    }
}
