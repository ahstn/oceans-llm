ALTER TABLE usage_cost_events ADD COLUMN uncached_input_tokens INTEGER CHECK (uncached_input_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN cache_read_tokens INTEGER CHECK (cache_read_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN cache_write_tokens INTEGER CHECK (cache_write_tokens >= 0);

CREATE TRIGGER usage_cost_events_cache_input_buckets_insert
BEFORE INSERT ON usage_cost_events
WHEN NEW.uncached_input_tokens IS NOT NULL
  AND NEW.cache_read_tokens IS NOT NULL
  AND NEW.cache_write_tokens IS NOT NULL
  AND NEW.prompt_tokens IS NOT NULL
  AND NEW.uncached_input_tokens + NEW.cache_read_tokens + NEW.cache_write_tokens != NEW.prompt_tokens
BEGIN
  SELECT RAISE(ABORT, 'cache input buckets must equal prompt_tokens');
END;

CREATE TRIGGER usage_cost_events_cache_input_buckets_update
BEFORE UPDATE OF uncached_input_tokens, cache_read_tokens, cache_write_tokens, prompt_tokens
ON usage_cost_events
WHEN NEW.uncached_input_tokens IS NOT NULL
  AND NEW.cache_read_tokens IS NOT NULL
  AND NEW.cache_write_tokens IS NOT NULL
  AND NEW.prompt_tokens IS NOT NULL
  AND NEW.uncached_input_tokens + NEW.cache_read_tokens + NEW.cache_write_tokens != NEW.prompt_tokens
BEGIN
  SELECT RAISE(ABORT, 'cache input buckets must equal prompt_tokens');
END;
