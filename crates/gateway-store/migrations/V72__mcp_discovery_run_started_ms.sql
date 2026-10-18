-- Discovery start times in milliseconds, so refreshes that start in the same
-- second are still ordered when a stale one is refused. Older rows keep NULL
-- and compare by their whole-second `started_at`.
ALTER TABLE external_mcp_discovery_runs ADD COLUMN started_at_ms INTEGER;
