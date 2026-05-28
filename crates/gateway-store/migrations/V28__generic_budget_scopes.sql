CREATE TABLE IF NOT EXISTS budgets (
  budget_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('user', 'service_account', 'user_model')),
  scope_key TEXT NOT NULL,
  user_id TEXT,
  service_account_id TEXT,
  model_id TEXT,
  upstream_model TEXT,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (
    (scope_kind = 'user'
      AND user_id IS NOT NULL
      AND service_account_id IS NULL
      AND model_id IS NULL
      AND upstream_model IS NULL
      AND scope_key = 'budget:v1:user:' || user_id)
    OR
    (scope_kind = 'service_account'
      AND service_account_id IS NOT NULL
      AND user_id IS NULL
      AND model_id IS NULL
      AND upstream_model IS NULL
      AND scope_key = 'budget:v1:service_account:' || service_account_id)
    OR
    (scope_kind = 'user_model'
      AND user_id IS NOT NULL
      AND service_account_id IS NULL
      AND (
        (model_id IS NOT NULL AND upstream_model IS NULL
          AND scope_key = 'budget:v1:user:' || user_id || ':model:' || model_id)
        OR
        (model_id IS NULL AND upstream_model IS NOT NULL AND TRIM(upstream_model) <> ''
          AND scope_key = 'budget:v1:user:' || user_id || ':upstream_model:' || TRIM(upstream_model))
      ))
  ),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS budgets_active_scope_uidx
  ON budgets (scope_key)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS budgets_scope_kind_idx
  ON budgets (scope_kind, is_active);

CREATE INDEX IF NOT EXISTS budgets_user_idx
  ON budgets (user_id);

CREATE INDEX IF NOT EXISTS budgets_service_account_idx
  ON budgets (service_account_id);

INSERT OR IGNORE INTO budgets (
  budget_id, scope_kind, scope_key, user_id, service_account_id, model_id, upstream_model,
  cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
)
SELECT
  user_budget_id,
  'user',
  'budget:v1:user:' || user_id,
  user_id,
  NULL,
  NULL,
  NULL,
  cadence,
  amount_10000,
  hard_limit,
  timezone,
  is_active,
  created_at,
  updated_at
FROM user_budgets
WHERE EXISTS (SELECT 1 FROM users WHERE users.user_id = user_budgets.user_id);

INSERT OR IGNORE INTO budgets (
  budget_id, scope_kind, scope_key, user_id, service_account_id, model_id, upstream_model,
  cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
)
SELECT
  service_account_budget_id,
  'service_account',
  'budget:v1:service_account:' || service_account_id,
  NULL,
  service_account_id,
  NULL,
  NULL,
  cadence,
  amount_10000,
  hard_limit,
  timezone,
  is_active,
  created_at,
  updated_at
FROM service_account_budgets
WHERE EXISTS (
  SELECT 1
  FROM service_accounts
  WHERE service_accounts.service_account_id = service_account_budgets.service_account_id
);

DELETE FROM budget_alert_deliveries
WHERE budget_alert_id IN (
  SELECT budget_alert_id
  FROM budget_alerts
  WHERE owner_kind = 'team'
);

DELETE FROM budget_alerts
WHERE owner_kind = 'team';

DELETE FROM usage_cost_events
WHERE ownership_scope_key LIKE 'team:%';

DELETE FROM usage_cost_event_duplicates_archive
WHERE ownership_scope_key LIKE 'team:%';

CREATE INDEX IF NOT EXISTS usage_cost_events_user_model_budget_idx
  ON usage_cost_events (user_id, model_id, occurred_at)
  WHERE user_id IS NOT NULL AND model_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS usage_cost_events_user_upstream_model_budget_idx
  ON usage_cost_events (user_id, TRIM(upstream_model), occurred_at)
  WHERE user_id IS NOT NULL AND model_id IS NULL AND upstream_model IS NOT NULL;

DELETE FROM api_key_model_grants
WHERE api_key_id IN (
  SELECT id
  FROM api_keys
  WHERE owner_kind = 'team'
);

DELETE FROM api_keys
WHERE owner_kind = 'team';

DROP TABLE IF EXISTS budget_alert_deliveries_v28;
DROP TABLE IF EXISTS budget_alerts_v28;

CREATE TABLE budget_alerts_v28 (
  budget_alert_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'service_account')),
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

INSERT INTO budget_alerts_v28 (
  budget_alert_id, ownership_scope_key, owner_kind, owner_id, owner_name, budget_id,
  cadence, threshold_bps, window_start, window_end, spend_before_10000,
  spend_after_10000, remaining_budget_10000, created_at, updated_at
)
SELECT
  budget_alert_id, ownership_scope_key, owner_kind, owner_id, owner_name, budget_id,
  cadence, threshold_bps, window_start, window_end, spend_before_10000,
  spend_after_10000, remaining_budget_10000, created_at, updated_at
FROM budget_alerts
WHERE owner_kind IN ('user', 'service_account');

CREATE TABLE budget_alert_deliveries_v28 (
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
  FOREIGN KEY (budget_alert_id) REFERENCES budget_alerts_v28(budget_alert_id) ON DELETE CASCADE
);

INSERT INTO budget_alert_deliveries_v28 (
  budget_alert_delivery_id, budget_alert_id, channel, delivery_status, recipient,
  provider_message_id, failure_reason, queued_at, last_attempted_at, sent_at, updated_at
)
SELECT
  delivery.budget_alert_delivery_id, delivery.budget_alert_id, delivery.channel,
  delivery.delivery_status, delivery.recipient, delivery.provider_message_id,
  delivery.failure_reason, delivery.queued_at, delivery.last_attempted_at,
  delivery.sent_at, delivery.updated_at
FROM budget_alert_deliveries AS delivery
JOIN budget_alerts_v28 AS alert
  ON alert.budget_alert_id = delivery.budget_alert_id;

DROP TABLE budget_alert_deliveries;
DROP TABLE budget_alerts;
ALTER TABLE budget_alerts_v28 RENAME TO budget_alerts;
ALTER TABLE budget_alert_deliveries_v28 RENAME TO budget_alert_deliveries;

CREATE UNIQUE INDEX IF NOT EXISTS budget_alerts_scope_threshold_window_uidx
  ON budget_alerts (ownership_scope_key, budget_id, threshold_bps, window_start);

CREATE INDEX IF NOT EXISTS budget_alerts_created_at_idx
  ON budget_alerts (created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alert_deliveries_alert_recipient_uidx
  ON budget_alert_deliveries (budget_alert_id, channel, recipient);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_status_idx
  ON budget_alert_deliveries (delivery_status, queued_at);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_alert_idx
  ON budget_alert_deliveries (budget_alert_id);

DELETE FROM mcp_tool_invocation_payloads
WHERE mcp_tool_invocation_id IN (
  SELECT mcp_tool_invocation_id
  FROM mcp_tool_invocations
  WHERE owner_kind = 'team'
);

DELETE FROM mcp_tool_invocations
WHERE owner_kind = 'team';

DROP TABLE IF EXISTS mcp_tool_invocation_payloads_v28;
DROP TABLE IF EXISTS mcp_tool_invocations_v28;

CREATE TABLE mcp_tool_invocations_v28 (
  mcp_tool_invocation_id TEXT PRIMARY KEY,
  request_log_id TEXT,
  request_id TEXT NOT NULL,
  api_key_id TEXT,
  user_id TEXT,
  team_id TEXT,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'service_account')),
  server_id TEXT,
  server_display_key TEXT NOT NULL,
  server_display_name TEXT NOT NULL,
  tool_id TEXT,
  tool_display_key TEXT NOT NULL,
  tool_display_name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('success', 'unauthorized', 'policy_denied', 'upstream_error', 'gateway_error', 'timeout', 'invalid_request')),
  policy_result TEXT NOT NULL CHECK (policy_result IN ('allowed', 'denied', 'not_evaluated')),
  latency_ms INTEGER,
  error_code TEXT,
  has_payload INTEGER NOT NULL DEFAULT 0 CHECK (has_payload IN (0, 1)),
  arguments_payload_truncated INTEGER NOT NULL DEFAULT 0 CHECK (arguments_payload_truncated IN (0, 1)),
  result_payload_truncated INTEGER NOT NULL DEFAULT 0 CHECK (result_payload_truncated IN (0, 1)),
  arguments_payload_redacted INTEGER NOT NULL DEFAULT 0 CHECK (arguments_payload_redacted IN (0, 1)),
  result_payload_redacted INTEGER NOT NULL DEFAULT 0 CHECK (result_payload_redacted IN (0, 1)),
  metadata_json TEXT NOT NULL DEFAULT '{}',
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL
);

INSERT INTO mcp_tool_invocations_v28
SELECT *
FROM mcp_tool_invocations
WHERE owner_kind IN ('user', 'service_account');

CREATE TABLE mcp_tool_invocation_payloads_v28 (
  mcp_tool_invocation_id TEXT PRIMARY KEY,
  arguments_json TEXT NOT NULL,
  result_json TEXT NOT NULL,
  FOREIGN KEY (mcp_tool_invocation_id)
    REFERENCES mcp_tool_invocations_v28(mcp_tool_invocation_id) ON DELETE CASCADE
);

INSERT INTO mcp_tool_invocation_payloads_v28
SELECT payload.*
FROM mcp_tool_invocation_payloads AS payload
JOIN mcp_tool_invocations_v28 AS invocation
  ON invocation.mcp_tool_invocation_id = payload.mcp_tool_invocation_id;

DROP TABLE mcp_tool_invocation_payloads;
DROP TABLE mcp_tool_invocations;
ALTER TABLE mcp_tool_invocations_v28 RENAME TO mcp_tool_invocations;
ALTER TABLE mcp_tool_invocation_payloads_v28 RENAME TO mcp_tool_invocation_payloads;

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_request_id_idx
  ON mcp_tool_invocations (request_id);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_request_log_idx
  ON mcp_tool_invocations (request_log_id);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_server_display_key_time_idx
  ON mcp_tool_invocations (server_display_key, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_server_display_name_time_idx
  ON mcp_tool_invocations (server_display_name, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_tool_display_key_time_idx
  ON mcp_tool_invocations (tool_display_key, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_tool_display_name_time_idx
  ON mcp_tool_invocations (tool_display_name, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_api_key_time_idx
  ON mcp_tool_invocations (api_key_id, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_user_time_idx
  ON mcp_tool_invocations (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_team_time_idx
  ON mcp_tool_invocations (team_id, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_status_time_idx
  ON mcp_tool_invocations (status, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_policy_time_idx
  ON mcp_tool_invocations (policy_result, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_occurred_at_idx
  ON mcp_tool_invocations (occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_recent_idx
  ON mcp_tool_invocations (occurred_at DESC, mcp_tool_invocation_id DESC);

DROP TABLE IF EXISTS team_budgets;
DROP TABLE IF EXISTS user_budgets;
DROP TABLE IF EXISTS service_account_budgets;
