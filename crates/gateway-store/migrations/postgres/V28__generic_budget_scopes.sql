CREATE TABLE IF NOT EXISTS budgets (
  budget_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('user', 'service_account', 'user_model')),
  scope_key TEXT NOT NULL,
  user_id TEXT,
  service_account_id TEXT,
  model_id TEXT,
  upstream_model TEXT,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 BIGINT NOT NULL CHECK (amount_10000 >= 0),
  hard_limit BIGINT NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active BIGINT NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
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

INSERT INTO budgets (
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
WHERE EXISTS (SELECT 1 FROM users WHERE users.user_id = user_budgets.user_id)
ON CONFLICT (budget_id) DO NOTHING;

INSERT INTO budgets (
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
)
ON CONFLICT (budget_id) DO NOTHING;

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

ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS api_keys_owner_kind_check;
ALTER TABLE api_keys
  ADD CONSTRAINT api_keys_owner_kind_check
  CHECK (owner_kind IN ('user', 'service_account'));

ALTER TABLE budget_alerts DROP CONSTRAINT IF EXISTS budget_alerts_owner_kind_check;
ALTER TABLE budget_alerts
  ADD CONSTRAINT budget_alerts_owner_kind_check
  CHECK (owner_kind IN ('user', 'service_account'));

DELETE FROM mcp_tool_invocation_payloads
WHERE mcp_tool_invocation_id IN (
  SELECT mcp_tool_invocation_id
  FROM mcp_tool_invocations
  WHERE owner_kind = 'team'
);

DELETE FROM mcp_tool_invocations
WHERE owner_kind = 'team';

ALTER TABLE mcp_tool_invocations DROP CONSTRAINT IF EXISTS mcp_tool_invocations_owner_kind_check;
ALTER TABLE mcp_tool_invocations
  ADD CONSTRAINT mcp_tool_invocations_owner_kind_check
  CHECK (owner_kind IN ('user', 'service_account'));

DROP TABLE IF EXISTS team_budgets;
DROP TABLE IF EXISTS user_budgets;
DROP TABLE IF EXISTS service_account_budgets;
