-- SQLite cannot alter a CHECK constraint in place, so the budgets table is
-- rebuilt with the widened source_kind domain. 'config_service_account' marks
-- budgets seeded from `service_accounts[*].budget`. Config reloads never
-- overwrite a manual admin edit to the active row; a manually deactivated
-- service-account budget is re-created because active keys require one.
CREATE TABLE budgets_v51 (
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
  source_kind TEXT NOT NULL DEFAULT 'manual'
    CHECK (source_kind IN ('manual', 'config_user_override', 'config_user_default', 'config_user_model_default', 'config_service_account')),
  source_key TEXT,
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

INSERT INTO budgets_v51 (
  budget_id, scope_kind, scope_key, user_id, service_account_id, model_id, upstream_model,
  cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at,
  source_kind, source_key
)
SELECT
  budget_id, scope_kind, scope_key, user_id, service_account_id, model_id, upstream_model,
  cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at,
  source_kind, source_key
FROM budgets;

DROP TABLE budgets;
ALTER TABLE budgets_v51 RENAME TO budgets;

CREATE UNIQUE INDEX IF NOT EXISTS budgets_active_scope_uidx
  ON budgets (scope_key)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS budgets_scope_kind_idx
  ON budgets (scope_kind, is_active);

CREATE INDEX IF NOT EXISTS budgets_user_idx
  ON budgets (user_id);

CREATE INDEX IF NOT EXISTS budgets_service_account_idx
  ON budgets (service_account_id);

CREATE INDEX IF NOT EXISTS budgets_source_idx
  ON budgets (source_kind, source_key, is_active);
