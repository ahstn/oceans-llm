ALTER TABLE user_budgets
  DROP CONSTRAINT IF EXISTS user_budgets_cadence_check;

ALTER TABLE user_budgets
  ADD CONSTRAINT user_budgets_cadence_check
  CHECK (cadence IN ('daily', 'weekly', 'monthly'));

ALTER TABLE team_budgets
  DROP CONSTRAINT IF EXISTS team_budgets_cadence_check;

ALTER TABLE team_budgets
  ADD CONSTRAINT team_budgets_cadence_check
  CHECK (cadence IN ('daily', 'weekly', 'monthly'));

CREATE TABLE IF NOT EXISTS budget_alerts (
  budget_alert_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team')),
  owner_id TEXT NOT NULL,
  owner_name TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  threshold_bps INTEGER NOT NULL CHECK (threshold_bps > 0 AND threshold_bps <= 10000),
  window_start BIGINT NOT NULL,
  window_end BIGINT NOT NULL,
  spend_before_10000 BIGINT NOT NULL CHECK (spend_before_10000 >= 0),
  spend_after_10000 BIGINT NOT NULL CHECK (spend_after_10000 >= 0),
  remaining_budget_10000 BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alerts_scope_threshold_window_uidx
  ON budget_alerts (ownership_scope_key, threshold_bps, window_start);

CREATE INDEX IF NOT EXISTS budget_alerts_created_at_idx
  ON budget_alerts (created_at DESC);

CREATE TABLE IF NOT EXISTS budget_alert_deliveries (
  budget_alert_delivery_id TEXT PRIMARY KEY,
  budget_alert_id TEXT NOT NULL REFERENCES budget_alerts(budget_alert_id) ON DELETE CASCADE,
  channel TEXT NOT NULL CHECK (channel IN ('email')),
  delivery_status TEXT NOT NULL CHECK (delivery_status IN ('pending', 'sent', 'failed')),
  recipient TEXT,
  provider_message_id TEXT,
  failure_reason TEXT,
  queued_at BIGINT NOT NULL,
  last_attempted_at BIGINT,
  sent_at BIGINT,
  updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alert_deliveries_alert_recipient_uidx
  ON budget_alert_deliveries (budget_alert_id, channel, recipient);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_pending_idx
  ON budget_alert_deliveries (delivery_status, queued_at);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_alert_idx
  ON budget_alert_deliveries (budget_alert_id);
