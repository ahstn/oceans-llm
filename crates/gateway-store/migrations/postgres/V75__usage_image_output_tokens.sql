SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS image_output_tokens BIGINT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS image_output_cost_per_million_tokens_10000 BIGINT;

-- Every existing row has a NULL image_output_tokens, which passes the check.
-- NOT VALID only skips re-proving that with a full-table scan while the
-- migration holds ACCESS EXCLUSIVE; new and updated rows are still checked.
ALTER TABLE usage_cost_events
  ADD CONSTRAINT usage_cost_events_image_output_tokens_check
  CHECK (image_output_tokens >= 0) NOT VALID;
