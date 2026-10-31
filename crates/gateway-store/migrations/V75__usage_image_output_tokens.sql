ALTER TABLE usage_cost_events ADD COLUMN image_output_tokens INTEGER CHECK (image_output_tokens >= 0);
ALTER TABLE usage_cost_events ADD COLUMN image_output_cost_per_million_tokens_10000 INTEGER;
