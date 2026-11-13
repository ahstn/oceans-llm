ALTER TABLE usage_cost_events ADD COLUMN cache_write_1h_tokens INTEGER CHECK (cache_write_1h_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN cache_write_1h_cost_per_million_tokens_10000 INTEGER;
ALTER TABLE model_pricing ADD COLUMN cache_write_1h_cost_per_million_tokens_10000 INTEGER;
