SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE model_pricing ADD COLUMN IF NOT EXISTS context_tiers_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE model_pricing ADD COLUMN IF NOT EXISTS output_image_cost_per_million_tokens_10000 BIGINT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS pricing_tier_above_input_tokens BIGINT;
