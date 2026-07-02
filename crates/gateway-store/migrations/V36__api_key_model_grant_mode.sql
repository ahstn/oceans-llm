ALTER TABLE api_keys
  ADD COLUMN model_grant_mode TEXT NOT NULL DEFAULT 'explicit'
  CHECK (model_grant_mode IN ('all', 'explicit'));
