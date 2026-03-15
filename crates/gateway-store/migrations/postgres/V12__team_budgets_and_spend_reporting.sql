CREATE TABLE IF NOT EXISTS team_budgets (
  team_budget_id TEXT PRIMARY KEY,
  team_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly')),
  amount_10000 BIGINT NOT NULL CHECK (amount_10000 >= 0),
  hard_limit BIGINT NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active BIGINT NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS team_budgets_active_team_uidx
  ON team_budgets (team_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS team_budgets_team_idx
  ON team_budgets (team_id);

CREATE INDEX IF NOT EXISTS usage_cost_events_occurred_at_idx
  ON usage_cost_events (occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_model_time_idx
  ON usage_cost_events (model_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_upstream_model_time_idx
  ON usage_cost_events (upstream_model, occurred_at);
