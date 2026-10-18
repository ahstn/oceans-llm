-- Discovery start times in milliseconds, so refreshes that start in the same
-- second are still ordered when a stale one is refused. Older rows are
-- backfilled from their whole-second `started_at`. Gateways older than this
-- migration, still serving during a rolling upgrade, insert runs without it;
-- the stale check treats those as newer than any run in the same second.
ALTER TABLE external_mcp_discovery_runs ADD COLUMN IF NOT EXISTS started_at_ms BIGINT;

UPDATE external_mcp_discovery_runs
SET started_at_ms = started_at * 1000
WHERE started_at_ms IS NULL;

-- The stale checks look for a newer run of a server, of any status or only
-- successful ones, so the index leads with the start rather than the status.
CREATE INDEX IF NOT EXISTS external_mcp_discovery_runs_server_started_ms_idx
  ON external_mcp_discovery_runs (mcp_server_id, started_at_ms, discovery_run_id);
