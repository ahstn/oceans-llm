ALTER TABLE model_routes ADD COLUMN context_window_tokens INTEGER;
ALTER TABLE model_routes ADD COLUMN pricing_override_json TEXT;

ALTER TABLE usage_cost_events ADD COLUMN model_route_id TEXT;
ALTER TABLE usage_cost_events ADD COLUMN cache_read_cost_per_million_tokens_10000 INTEGER;
ALTER TABLE usage_cost_events ADD COLUMN cache_write_cost_per_million_tokens_10000 INTEGER;
