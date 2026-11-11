ALTER TABLE model_pricing ADD COLUMN service_tiers_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE usage_cost_events ADD COLUMN requested_service_tier TEXT;
ALTER TABLE usage_cost_events ADD COLUMN served_service_tier TEXT;
ALTER TABLE gateway_models ADD COLUMN service_tiers_json TEXT;
ALTER TABLE model_routes ADD COLUMN service_tier TEXT;
