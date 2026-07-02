ALTER TABLE api_keys
  ADD COLUMN IF NOT EXISTS model_grant_mode TEXT NOT NULL DEFAULT 'explicit';

UPDATE api_keys
SET model_grant_mode = 'explicit'
WHERE model_grant_mode IS NULL;

ALTER TABLE api_keys
  DROP CONSTRAINT IF EXISTS api_keys_model_grant_mode_check;

ALTER TABLE api_keys
  ADD CONSTRAINT api_keys_model_grant_mode_check
  CHECK (model_grant_mode IN ('all', 'explicit'));
