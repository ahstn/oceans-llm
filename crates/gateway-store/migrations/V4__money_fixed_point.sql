PRAGMA foreign_keys = OFF;

-- Compatibility: some older test databases may not include V3 identity tables.
-- Create legacy-shaped tables when missing so this migration is still safe to apply.
CREATE TABLE IF NOT EXISTS user_budgets (
  user_budget_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  cadence TEXT NOT NULL,
  amount_usd REAL NOT NULL,
  hard_limit INTEGER NOT NULL DEFAULT 1,
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS usage_cost_events (
  usage_event_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_id TEXT,
  estimated_cost_usd REAL NOT NULL,
  occurred_at INTEGER NOT NULL
);

CREATE TABLE user_budgets_v4 (
  user_budget_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

INSERT INTO user_budgets_v4 (
  user_budget_id,
  user_id,
  cadence,
  amount_10000,
  hard_limit,
  timezone,
  is_active,
  created_at,
  updated_at
)
SELECT
  user_budget_id,
  user_id,
  cadence,
  CAST(ROUND(amount_usd * 10000.0) AS INTEGER),
  hard_limit,
  timezone,
  is_active,
  created_at,
  updated_at
FROM user_budgets;

DROP TABLE user_budgets;
ALTER TABLE user_budgets_v4 RENAME TO user_budgets;

CREATE UNIQUE INDEX IF NOT EXISTS user_budgets_active_user_uidx
  ON user_budgets (user_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS user_budgets_user_idx
  ON user_budgets (user_id);

CREATE TABLE usage_cost_events_v4 (
  usage_event_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_id TEXT,
  estimated_cost_10000 INTEGER NOT NULL CHECK (estimated_cost_10000 >= 0),
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE SET NULL
);

INSERT INTO usage_cost_events_v4 (
  usage_event_id,
  request_id,
  api_key_id,
  user_id,
  team_id,
  model_id,
  estimated_cost_10000,
  occurred_at
)
SELECT
  usage_event_id,
  request_id,
  api_key_id,
  user_id,
  team_id,
  model_id,
  CAST(ROUND(estimated_cost_usd * 10000.0) AS INTEGER),
  occurred_at
FROM usage_cost_events;

DROP TABLE usage_cost_events;
ALTER TABLE usage_cost_events_v4 RENAME TO usage_cost_events;

CREATE INDEX IF NOT EXISTS usage_cost_events_user_time_idx
  ON usage_cost_events (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_team_time_idx
  ON usage_cost_events (team_id, occurred_at);

PRAGMA foreign_keys = ON;
