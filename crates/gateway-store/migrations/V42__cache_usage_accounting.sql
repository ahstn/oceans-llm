ALTER TABLE usage_cost_events ADD COLUMN uncached_input_tokens INTEGER CHECK (uncached_input_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN cache_read_tokens INTEGER CHECK (cache_read_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN cache_write_tokens INTEGER CHECK (cache_write_tokens >= 0);

CREATE INDEX usage_cost_events_cache_usage_idx
  ON usage_cost_events (occurred_at, cache_read_tokens, cache_write_tokens);
