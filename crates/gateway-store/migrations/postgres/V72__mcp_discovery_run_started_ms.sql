-- Discovery start times in milliseconds, so refreshes that start in the same
-- second are still ordered when a stale one is refused. Older rows are
-- backfilled from their whole-second `started_at`, and every insert sets the
-- column, so the stale check reads it alone and stays on the index.
ALTER TABLE external_mcp_discovery_runs ADD COLUMN IF NOT EXISTS started_at_ms BIGINT;

UPDATE external_mcp_discovery_runs
SET started_at_ms = started_at * 1000
WHERE started_at_ms IS NULL;

CREATE INDEX IF NOT EXISTS external_mcp_discovery_runs_server_status_ms_idx
  ON external_mcp_discovery_runs (mcp_server_id, status, started_at_ms);
