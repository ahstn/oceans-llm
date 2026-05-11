CREATE TABLE IF NOT EXISTS service_accounts (
  service_account_id TEXT PRIMARY KEY,
  team_id TEXT NOT NULL,
  service_account_key TEXT NOT NULL UNIQUE,
  service_account_name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
  model_access_mode TEXT NOT NULL DEFAULT 'all' CHECK (model_access_mode IN ('all', 'restricted')),
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  disabled_at BIGINT,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS service_accounts_team_idx
  ON service_accounts (team_id);

CREATE INDEX IF NOT EXISTS service_accounts_status_idx
  ON service_accounts (status);

CREATE TABLE IF NOT EXISTS service_account_model_allowlist (
  service_account_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (service_account_id, model_id),
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS service_account_model_allowlist_model_idx
  ON service_account_model_allowlist (model_id);

ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS api_keys_owner_kind_check;
ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS api_keys_check;
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS owner_service_account_id TEXT;

DELETE FROM api_keys
WHERE owner_kind = 'team';

ALTER TABLE api_keys
  DROP CONSTRAINT IF EXISTS api_keys_owner_team_id_fkey;

ALTER TABLE api_keys
  ADD CONSTRAINT api_keys_owner_team_id_fkey
  FOREIGN KEY (owner_team_id) REFERENCES teams(team_id) ON DELETE SET NULL;

ALTER TABLE api_keys
  DROP CONSTRAINT IF EXISTS api_keys_owner_service_account_id_fkey;

ALTER TABLE api_keys
  ADD CONSTRAINT api_keys_owner_service_account_id_fkey
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE;

ALTER TABLE api_keys
  ADD CONSTRAINT api_keys_owner_kind_check
  CHECK (owner_kind IN ('user', 'service_account'));

ALTER TABLE api_keys
  ADD CONSTRAINT api_keys_owner_check
  CHECK (
    (owner_kind = 'user' AND owner_user_id IS NOT NULL AND owner_team_id IS NULL AND owner_service_account_id IS NULL) OR
    (owner_kind = 'service_account' AND owner_service_account_id IS NOT NULL AND owner_user_id IS NULL)
  );

DROP INDEX IF EXISTS api_keys_owner_team_idx;

CREATE INDEX IF NOT EXISTS api_keys_owner_service_account_idx
  ON api_keys (owner_service_account_id);

ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS service_account_id TEXT;

ALTER TABLE request_logs
  DROP CONSTRAINT IF EXISTS request_logs_service_account_id_fkey;

ALTER TABLE request_logs
  ADD CONSTRAINT request_logs_service_account_id_fkey
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS request_logs_service_account_time_idx
  ON request_logs (service_account_id, occurred_at);

CREATE TABLE IF NOT EXISTS service_account_budgets (
  service_account_budget_id TEXT PRIMARY KEY,
  service_account_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 BIGINT NOT NULL CHECK (amount_10000 >= 0),
  hard_limit BIGINT NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active BIGINT NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS service_account_budgets_active_service_account_uidx
  ON service_account_budgets (service_account_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS service_account_budgets_service_account_idx
  ON service_account_budgets (service_account_id);

ALTER TABLE budget_alerts DROP CONSTRAINT IF EXISTS budget_alerts_owner_kind_check;
ALTER TABLE budget_alerts
  ADD CONSTRAINT budget_alerts_owner_kind_check
  CHECK (owner_kind IN ('user', 'team', 'service_account'));

ALTER TABLE usage_cost_event_duplicates_archive ADD COLUMN IF NOT EXISTS service_account_id TEXT;
ALTER TABLE usage_cost_events ADD COLUMN IF NOT EXISTS service_account_id TEXT;

ALTER TABLE usage_cost_events
  DROP CONSTRAINT IF EXISTS usage_cost_events_service_account_id_fkey;

ALTER TABLE usage_cost_events
  ADD CONSTRAINT usage_cost_events_service_account_id_fkey
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS usage_cost_events_service_account_time_idx
  ON usage_cost_events (service_account_id, occurred_at);
