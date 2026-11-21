-- Budget checks add up a scope's priced spend over its budget window on every priced
-- request: once before the provider call and once after the ledger insert. The indexes
-- they used found a scope's rows but held neither the cost nor the pricing status, so
-- every check read each row from the table. SQLite has no INCLUDE, so these indexes end
-- with both columns as keys, and the sum reads only the index. Each one starts with the
-- key of the index it replaces, so the queries that used the old index use it.
CREATE INDEX IF NOT EXISTS usage_cost_events_user_spend_idx
  ON usage_cost_events (user_id, occurred_at, pricing_status, computed_cost_10000);

CREATE INDEX IF NOT EXISTS usage_cost_events_service_account_spend_idx
  ON usage_cost_events (service_account_id, occurred_at, pricing_status, computed_cost_10000);

CREATE INDEX IF NOT EXISTS usage_cost_events_user_model_spend_idx
  ON usage_cost_events (user_id, model_id, occurred_at, pricing_status, computed_cost_10000)
  WHERE user_id IS NOT NULL AND model_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS usage_cost_events_user_upstream_model_spend_idx
  ON usage_cost_events (
    user_id, TRIM(upstream_model), occurred_at, pricing_status, computed_cost_10000
  )
  WHERE user_id IS NOT NULL AND model_id IS NULL AND upstream_model IS NOT NULL;

DROP INDEX IF EXISTS usage_cost_events_user_time_idx;
DROP INDEX IF EXISTS usage_cost_events_service_account_time_idx;
DROP INDEX IF EXISTS usage_cost_events_user_model_budget_idx;
DROP INDEX IF EXISTS usage_cost_events_user_upstream_model_budget_idx;
