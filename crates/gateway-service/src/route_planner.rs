use std::{collections::BTreeMap, sync::Mutex};

use gateway_core::{ModelRoute, RouteError, RoutePlanner};
use rand::{
    SeedableRng,
    distributions::{Distribution, WeightedIndex},
    rngs::StdRng,
};

pub struct WeightedRoutePlanner {
    rng: Mutex<StdRng>,
}

impl WeightedRoutePlanner {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rng: Mutex::new(StdRng::from_entropy()),
        }
    }

    #[must_use]
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
        }
    }
}

impl Default for WeightedRoutePlanner {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutePlanner for WeightedRoutePlanner {
    fn plan_routes(&self, routes: &[ModelRoute]) -> Result<Vec<ModelRoute>, RouteError> {
        let mut by_priority: BTreeMap<i32, Vec<ModelRoute>> = BTreeMap::new();

        for route in routes {
            if route.enabled && route.weight > 0.0 {
                by_priority
                    .entry(route.priority)
                    .or_default()
                    .push(route.clone());
            }
        }

        if by_priority.is_empty() {
            return Err(RouteError::NoRoutesAvailable("requested model".to_string()));
        }

        let mut planned = Vec::new();
        let mut rng = self
            .rng
            .lock()
            .map_err(|_| RouteError::Policy("route planner RNG lock poisoned".to_string()))?;

        for mut same_priority in by_priority.into_values() {
            while !same_priority.is_empty() {
                let weights = same_priority
                    .iter()
                    .map(|route| route.weight.max(f64::EPSILON))
                    .collect::<Vec<_>>();
                let distribution = WeightedIndex::new(weights).map_err(|error| {
                    RouteError::Policy(format!("invalid route weights for selection: {error}"))
                })?;
                let index = distribution.sample(&mut *rng);
                planned.push(same_priority.swap_remove(index));
            }
        }

        Ok(planned)
    }
}

#[cfg(test)]
mod tests {
    use gateway_core::{ModelRoute, ProviderCapabilities, RoutePlanner};
    use serde_json::Map;
    use uuid::Uuid;

    use super::WeightedRoutePlanner;

    fn route(priority: i32, weight: f64, provider_key: &str) -> ModelRoute {
        ModelRoute {
            id: Uuid::new_v4(),
            model_id: Uuid::new_v4(),
            provider_key: provider_key.to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            priority,
            weight,
            enabled: true,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            capabilities: ProviderCapabilities::all_enabled(),
            compatibility: Default::default(),
        }
    }

    #[test]
    fn plans_routes_by_priority_then_weighted_selection() {
        let planner = WeightedRoutePlanner::seeded(7);
        let routes = vec![
            route(20, 1.0, "fallback"),
            route(10, 9.0, "primary-heavy"),
            route(10, 1.0, "primary-light"),
        ];

        let planned = planner.plan_routes(&routes).expect("plan routes");
        assert_eq!(planned.len(), 3);

        let first_priority = planned.first().expect("first route").priority;
        let second_priority = planned.get(1).expect("second route").priority;
        let third_priority = planned.get(2).expect("third route").priority;

        assert_eq!(first_priority, 10);
        assert_eq!(second_priority, 10);
        assert_eq!(third_priority, 20);
    }
}
