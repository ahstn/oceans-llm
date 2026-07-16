ALTER TABLE model_routes ADD COLUMN IF NOT EXISTS context_window_tokens BIGINT;
ALTER TABLE model_routes ADD COLUMN IF NOT EXISTS pricing_override_json TEXT;

ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS model_route_id TEXT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS cache_read_cost_per_million_tokens_10000 BIGINT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS cache_write_cost_per_million_tokens_10000 BIGINT;
