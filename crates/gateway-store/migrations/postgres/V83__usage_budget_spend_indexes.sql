SET LOCAL lock_timeout = '5s';
-- Longer than other migrations' 60s: each index build reads every usage_cost_events row.
SET LOCAL statement_timeout = '15min';

-- Budget checks add up a scope's priced spend over its budget window on every priced
-- request: once before the provider call and once after the ledger insert. The indexes
-- they used found a scope's rows but held neither the cost nor the pricing status, so
-- every check read each row from the table, and a heavy user's check scanned the whole
-- table. These indexes carry both columns, so the sum reads only the index. Each one
-- keeps the key of the index it replaces, so the queries that used the old index use it.
--
-- The runner applies each migration in a transaction, so these builds cannot be
-- CONCURRENTLY and block writes to usage_cost_events, and so priced requests, until they
-- finish. On a large table, first run each CREATE INDEX below with CONCURRENTLY, one at a
-- time and outside a transaction, and check that `pg_index.indisvalid` is true for each
-- (a failed concurrent build leaves an invalid index that this migration would skip).
-- The migration then only drops the old indexes.
CREATE INDEX IF NOT EXISTS usage_cost_events_user_spend_idx
  ON usage_cost_events (user_id, occurred_at)
  INCLUDE (pricing_status, computed_cost_10000);

CREATE INDEX IF NOT EXISTS usage_cost_events_service_account_spend_idx
  ON usage_cost_events (service_account_id, occurred_at)
  INCLUDE (pricing_status, computed_cost_10000);

CREATE INDEX IF NOT EXISTS usage_cost_events_user_model_spend_idx
  ON usage_cost_events (user_id, model_id, occurred_at)
  INCLUDE (pricing_status, computed_cost_10000)
  WHERE user_id IS NOT NULL AND model_id IS NOT NULL;

-- An index-only scan needs the columns an index expression reads, so this one also
-- carries `upstream_model` for `TRIM(upstream_model)`.
CREATE INDEX IF NOT EXISTS usage_cost_events_user_upstream_model_spend_idx
  ON usage_cost_events (user_id, TRIM(upstream_model), occurred_at)
  INCLUDE (pricing_status, computed_cost_10000, upstream_model)
  WHERE user_id IS NOT NULL AND model_id IS NULL AND upstream_model IS NOT NULL;

DROP INDEX IF EXISTS usage_cost_events_user_time_idx;
DROP INDEX IF EXISTS usage_cost_events_service_account_time_idx;
DROP INDEX IF EXISTS usage_cost_events_user_model_budget_idx;
DROP INDEX IF EXISTS usage_cost_events_user_upstream_model_budget_idx;
