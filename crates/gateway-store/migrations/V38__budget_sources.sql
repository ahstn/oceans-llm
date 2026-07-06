ALTER TABLE budgets
  ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'manual'
  CHECK (source_kind IN ('manual', 'config_user_override', 'config_user_default', 'config_user_model_default'));

ALTER TABLE budgets
  ADD COLUMN source_key TEXT;

UPDATE budgets
SET source_key = 'deactivated'
WHERE is_active = 0
  AND source_kind = 'manual'
  AND source_key IS NULL;

CREATE INDEX IF NOT EXISTS budgets_source_idx
  ON budgets (source_kind, source_key, is_active);
