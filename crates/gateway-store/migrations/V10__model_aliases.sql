ALTER TABLE gateway_models
  ADD COLUMN alias_target_model_id TEXT REFERENCES gateway_models(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS gateway_models_alias_target_idx
  ON gateway_models (alias_target_model_id);
