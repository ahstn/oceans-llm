ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS uncached_input_tokens BIGINT CHECK (uncached_input_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT CHECK (cache_read_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS cache_write_tokens BIGINT CHECK (cache_write_tokens >= 0);

ALTER TABLE usage_cost_events
  ADD CONSTRAINT usage_cost_events_cache_input_buckets_check
  CHECK (
    uncached_input_tokens IS NULL
    OR cache_read_tokens IS NULL
    OR cache_write_tokens IS NULL
    OR prompt_tokens IS NULL
    OR uncached_input_tokens + cache_read_tokens + cache_write_tokens = prompt_tokens
  ) NOT VALID;

ALTER TABLE usage_cost_events
  VALIDATE CONSTRAINT usage_cost_events_cache_input_buckets_check;
