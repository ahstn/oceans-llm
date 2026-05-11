CREATE TABLE IF NOT EXISTS service_accounts (
  service_account_id TEXT PRIMARY KEY,
  team_id TEXT NOT NULL,
  service_account_key TEXT NOT NULL UNIQUE,
  service_account_name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
  model_access_mode TEXT NOT NULL DEFAULT 'all' CHECK (model_access_mode IN ('all', 'restricted')),
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  disabled_at INTEGER,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS service_accounts_team_idx
  ON service_accounts (team_id);

CREATE INDEX IF NOT EXISTS service_accounts_status_idx
  ON service_accounts (status);

CREATE UNIQUE INDEX IF NOT EXISTS service_accounts_id_team_uidx
  ON service_accounts (service_account_id, team_id);

CREATE TABLE IF NOT EXISTS service_account_model_allowlist (
  service_account_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (service_account_id, model_id),
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS service_account_model_allowlist_model_idx
  ON service_account_model_allowlist (model_id);

DROP TABLE IF EXISTS api_keys_new;

DROP TABLE IF EXISTS v21_api_key_model_grants_preserved;
CREATE TEMP TABLE v21_api_key_model_grants_preserved AS
SELECT grant_row.*
FROM api_key_model_grants AS grant_row
JOIN api_keys AS key_row ON key_row.id = grant_row.api_key_id
WHERE key_row.owner_kind = 'user';

DROP TABLE IF EXISTS v21_request_logs_preserved;
CREATE TEMP TABLE v21_request_logs_preserved AS
SELECT log_row.*
FROM request_logs AS log_row
JOIN api_keys AS key_row ON key_row.id = log_row.api_key_id
WHERE key_row.owner_kind = 'user';

DROP TABLE IF EXISTS v21_request_log_payloads_preserved;
CREATE TEMP TABLE v21_request_log_payloads_preserved AS
SELECT payload_row.*
FROM request_log_payloads AS payload_row
JOIN v21_request_logs_preserved AS log_row ON log_row.request_log_id = payload_row.request_log_id;

DROP TABLE IF EXISTS v21_request_log_tags_preserved;
CREATE TEMP TABLE v21_request_log_tags_preserved AS
SELECT tag_row.*
FROM request_log_tags AS tag_row
JOIN v21_request_logs_preserved AS log_row ON log_row.request_log_id = tag_row.request_log_id;

DROP TABLE IF EXISTS v21_request_log_attempts_preserved;
CREATE TEMP TABLE v21_request_log_attempts_preserved AS
SELECT attempt_row.*
FROM request_log_attempts AS attempt_row
JOIN v21_request_logs_preserved AS log_row ON log_row.request_log_id = attempt_row.request_log_id;

DROP TABLE IF EXISTS v21_usage_cost_events_preserved;
CREATE TEMP TABLE v21_usage_cost_events_preserved AS
SELECT event_row.*
FROM usage_cost_events AS event_row
JOIN api_keys AS key_row ON key_row.id = event_row.api_key_id
WHERE key_row.owner_kind = 'user';

DROP TABLE IF EXISTS v21_audit_log_actor_keys_preserved;
CREATE TEMP TABLE v21_audit_log_actor_keys_preserved AS
SELECT audit_row.id, audit_row.actor_api_key_id
FROM audit_logs AS audit_row
JOIN api_keys AS key_row ON key_row.id = audit_row.actor_api_key_id
WHERE key_row.owner_kind = 'user';

CREATE TABLE api_keys_new (
  id TEXT PRIMARY KEY,
  public_id TEXT NOT NULL UNIQUE,
  secret_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'service_account')),
  owner_user_id TEXT,
  owner_team_id TEXT,
  owner_service_account_id TEXT,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  revoked_at INTEGER,
  CHECK (
    (owner_kind = 'user' AND owner_user_id IS NOT NULL AND owner_team_id IS NULL AND owner_service_account_id IS NULL) OR
    (owner_kind = 'service_account' AND owner_service_account_id IS NOT NULL AND owner_team_id IS NOT NULL AND owner_user_id IS NULL)
  ),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_service_account_id, owner_team_id)
    REFERENCES service_accounts(service_account_id, team_id) ON DELETE CASCADE
);

INSERT INTO api_keys_new (
  id, public_id, secret_hash, name, status, owner_kind, owner_user_id, owner_team_id,
  owner_service_account_id, created_at, last_used_at, revoked_at
)
SELECT id, public_id, secret_hash, name, status,
       owner_kind,
       owner_user_id, owner_team_id,
       NULL,
       created_at, last_used_at, revoked_at
FROM api_keys
WHERE owner_kind = 'user';

DROP TABLE api_keys;
ALTER TABLE api_keys_new RENAME TO api_keys;

INSERT INTO api_key_model_grants
SELECT * FROM v21_api_key_model_grants_preserved;

INSERT INTO request_logs
SELECT * FROM v21_request_logs_preserved;

INSERT INTO request_log_payloads
SELECT * FROM v21_request_log_payloads_preserved;

INSERT INTO request_log_tags
SELECT * FROM v21_request_log_tags_preserved;

INSERT INTO request_log_attempts
SELECT * FROM v21_request_log_attempts_preserved;

INSERT INTO usage_cost_events
SELECT * FROM v21_usage_cost_events_preserved;

UPDATE audit_logs
SET actor_api_key_id = (
  SELECT preserved.actor_api_key_id
  FROM v21_audit_log_actor_keys_preserved AS preserved
  WHERE preserved.id = audit_logs.id
)
WHERE id IN (SELECT id FROM v21_audit_log_actor_keys_preserved);

DROP TABLE v21_api_key_model_grants_preserved;
DROP TABLE v21_request_log_payloads_preserved;
DROP TABLE v21_request_log_tags_preserved;
DROP TABLE v21_request_log_attempts_preserved;
DROP TABLE v21_request_logs_preserved;
DROP TABLE v21_usage_cost_events_preserved;
DROP TABLE v21_audit_log_actor_keys_preserved;

CREATE INDEX IF NOT EXISTS api_keys_owner_user_idx
  ON api_keys (owner_user_id);

CREATE INDEX IF NOT EXISTS api_keys_owner_service_account_idx
  ON api_keys (owner_service_account_id);

ALTER TABLE request_logs ADD COLUMN service_account_id TEXT;

CREATE INDEX IF NOT EXISTS request_logs_service_account_time_idx
  ON request_logs (service_account_id, occurred_at);

CREATE TABLE IF NOT EXISTS service_account_budgets (
  service_account_budget_id TEXT PRIMARY KEY,
  service_account_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS service_account_budgets_active_service_account_uidx
  ON service_account_budgets (service_account_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS service_account_budgets_service_account_idx
  ON service_account_budgets (service_account_id);

DROP TABLE IF EXISTS budget_alerts_new;

CREATE TABLE budget_alerts_new (
  budget_alert_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team', 'service_account')),
  owner_id TEXT NOT NULL,
  owner_name TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  threshold_bps INTEGER NOT NULL CHECK (threshold_bps > 0 AND threshold_bps <= 10000),
  window_start INTEGER NOT NULL,
  window_end INTEGER NOT NULL,
  spend_before_10000 INTEGER NOT NULL CHECK (spend_before_10000 >= 0),
  spend_after_10000 INTEGER NOT NULL CHECK (spend_after_10000 >= 0),
  remaining_budget_10000 INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

INSERT INTO budget_alerts_new (
  budget_alert_id, ownership_scope_key, owner_kind, owner_id, owner_name, budget_id,
  cadence, threshold_bps, window_start, window_end, spend_before_10000,
  spend_after_10000, remaining_budget_10000, created_at, updated_at
)
SELECT budget_alert_id, ownership_scope_key, owner_kind, owner_id, owner_name, budget_id,
       cadence, threshold_bps, window_start, window_end, spend_before_10000,
       spend_after_10000, remaining_budget_10000, created_at, updated_at
FROM budget_alerts;

DROP TABLE IF EXISTS budget_alert_deliveries_new;

CREATE TABLE budget_alert_deliveries_new (
  budget_alert_delivery_id TEXT PRIMARY KEY,
  budget_alert_id TEXT NOT NULL,
  channel TEXT NOT NULL CHECK (channel IN ('email')),
  delivery_status TEXT NOT NULL CHECK (delivery_status IN ('pending', 'sent', 'failed')),
  recipient TEXT,
  provider_message_id TEXT,
  failure_reason TEXT,
  queued_at INTEGER NOT NULL,
  last_attempted_at INTEGER,
  sent_at INTEGER,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (budget_alert_id) REFERENCES budget_alerts_new(budget_alert_id) ON DELETE CASCADE
);

INSERT INTO budget_alert_deliveries_new (
  budget_alert_delivery_id, budget_alert_id, channel, delivery_status, recipient,
  provider_message_id, failure_reason, queued_at, last_attempted_at, sent_at, updated_at
)
SELECT budget_alert_delivery_id, budget_alert_id, channel, delivery_status, recipient,
       provider_message_id, failure_reason, queued_at, last_attempted_at, sent_at, updated_at
FROM budget_alert_deliveries;

DROP TABLE budget_alert_deliveries;
DROP TABLE budget_alerts;

ALTER TABLE budget_alerts_new RENAME TO budget_alerts;
ALTER TABLE budget_alert_deliveries_new RENAME TO budget_alert_deliveries;

CREATE UNIQUE INDEX IF NOT EXISTS budget_alerts_scope_threshold_window_uidx
  ON budget_alerts (ownership_scope_key, budget_id, threshold_bps, window_start);

CREATE INDEX IF NOT EXISTS budget_alerts_created_at_idx
  ON budget_alerts (created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alert_deliveries_alert_recipient_uidx
  ON budget_alert_deliveries (budget_alert_id, channel, recipient);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_status_idx
  ON budget_alert_deliveries (delivery_status, queued_at);

ALTER TABLE usage_cost_event_duplicates_archive ADD COLUMN service_account_id TEXT;
ALTER TABLE usage_cost_events ADD COLUMN service_account_id TEXT;

CREATE INDEX IF NOT EXISTS usage_cost_events_service_account_time_idx
  ON usage_cost_events (service_account_id, occurred_at);
