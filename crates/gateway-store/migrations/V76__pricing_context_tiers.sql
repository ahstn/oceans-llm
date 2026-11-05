ALTER TABLE model_pricing ADD COLUMN context_tiers_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE model_pricing ADD COLUMN output_image_cost_per_million_tokens_10000 INTEGER;
ALTER TABLE usage_cost_events ADD COLUMN pricing_tier_above_input_tokens INTEGER;
