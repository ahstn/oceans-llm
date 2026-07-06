ALTER TABLE budgets
  ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'manual';

ALTER TABLE budgets
  ADD COLUMN source_key TEXT;

ALTER TABLE budgets
  ADD CONSTRAINT budgets_source_kind_check
  CHECK (source_kind IN ('manual', 'config_user_override', 'config_user_default', 'config_user_model_default'));

CREATE INDEX IF NOT EXISTS budgets_source_idx
  ON budgets (source_kind, source_key, is_active);
