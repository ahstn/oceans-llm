CREATE TABLE user_budgets_v14 (
  user_budget_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

INSERT INTO user_budgets_v14 (
  user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
)
SELECT
  user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
FROM user_budgets;

DROP TABLE user_budgets;
ALTER TABLE user_budgets_v14 RENAME TO user_budgets;

CREATE UNIQUE INDEX IF NOT EXISTS user_budgets_active_user_uidx
  ON user_budgets (user_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS user_budgets_user_idx
  ON user_budgets (user_id);

CREATE TABLE team_budgets_v14 (
  team_budget_id TEXT PRIMARY KEY,
  team_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

INSERT INTO team_budgets_v14 (
  team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
)
SELECT
  team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
FROM team_budgets;

DROP TABLE team_budgets;
ALTER TABLE team_budgets_v14 RENAME TO team_budgets;

CREATE UNIQUE INDEX IF NOT EXISTS team_budgets_active_team_uidx
  ON team_budgets (team_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS team_budgets_team_idx
  ON team_budgets (team_id);

CREATE TABLE IF NOT EXISTS budget_alerts (
  budget_alert_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team')),
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

CREATE UNIQUE INDEX IF NOT EXISTS budget_alerts_scope_threshold_window_uidx
  ON budget_alerts (ownership_scope_key, threshold_bps, window_start);

CREATE INDEX IF NOT EXISTS budget_alerts_created_at_idx
  ON budget_alerts (created_at DESC);

CREATE TABLE IF NOT EXISTS budget_alert_deliveries (
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
  FOREIGN KEY (budget_alert_id) REFERENCES budget_alerts(budget_alert_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alert_deliveries_alert_recipient_uidx
  ON budget_alert_deliveries (budget_alert_id, channel, recipient);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_pending_idx
  ON budget_alert_deliveries (delivery_status, queued_at);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_alert_idx
  ON budget_alert_deliveries (budget_alert_id);
