-- Temporary increases to an active budget's limit. Enforcement adds the
-- increase active at the request time to the base amount. Creating a new
-- increase supersedes any increase still active for the same budget, so at
-- most one row per budget applies at a time.
CREATE TABLE IF NOT EXISTS budget_increases (
  budget_increase_id TEXT PRIMARY KEY,
  budget_id TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  amount_10000 BIGINT NOT NULL CHECK (amount_10000 > 0),
  starts_at BIGINT NOT NULL,
  expires_at BIGINT NOT NULL,
  created_by_user_id TEXT,
  created_at BIGINT NOT NULL,
  superseded_at BIGINT,
  CHECK (expires_at > starts_at),
  FOREIGN KEY (budget_id) REFERENCES budgets(budget_id) ON DELETE CASCADE,
  FOREIGN KEY (created_by_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS budget_increases_budget_expiry_idx
  ON budget_increases (budget_id, expires_at);

CREATE INDEX IF NOT EXISTS budget_increases_expiry_idx
  ON budget_increases (expires_at);
