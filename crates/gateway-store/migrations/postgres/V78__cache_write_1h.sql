SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS cache_write_1h_tokens BIGINT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS cache_write_1h_cost_per_million_tokens_10000 BIGINT;
ALTER TABLE model_pricing ADD COLUMN IF NOT EXISTS cache_write_1h_cost_per_million_tokens_10000 BIGINT;

-- Every existing row has a NULL cache_write_1h_tokens, which passes the check.
-- NOT VALID only skips re-proving that with a full-table scan while the
-- migration holds ACCESS EXCLUSIVE; new and updated rows are still checked.
ALTER TABLE usage_cost_events
  ADD CONSTRAINT usage_cost_events_cache_write_1h_tokens_check
  CHECK (cache_write_1h_tokens >= 0) NOT VALID;
