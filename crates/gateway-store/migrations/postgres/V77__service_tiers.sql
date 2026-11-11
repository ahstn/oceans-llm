SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE model_pricing ADD COLUMN IF NOT EXISTS service_tiers_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS requested_service_tier TEXT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS served_service_tier TEXT;
ALTER TABLE gateway_models ADD COLUMN IF NOT EXISTS service_tiers_json TEXT;
ALTER TABLE model_routes ADD COLUMN IF NOT EXISTS service_tier TEXT;
