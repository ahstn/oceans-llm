-- Admin-managed rate-limit policies. Config-defined policies are held in
-- memory and are never written here. Every enabled policy that matches a
-- request applies; the most restrictive one rejects it.
CREATE TABLE IF NOT EXISTS rate_limit_policies (
  rate_limit_policy_id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  operation TEXT NOT NULL CHECK (operation IN ('inference', 'count_tokens', 'mcp_tool_call')),
  scope_kind TEXT NOT NULL CHECK (scope_kind IN ('global', 'api_key', 'user', 'team')),
  -- NULL gives each principal of scope_kind its own counter.
  scope_id TEXT,
  model_key TEXT,
  mcp_server_id TEXT,
  metric TEXT NOT NULL CHECK (metric IN ('requests', 'tokens')),
  period TEXT NOT NULL CHECK (period IN ('minute', 'hour', 'day', 'month')),
  limit_value INTEGER NOT NULL CHECK (limit_value > 0),
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (scope_kind <> 'global' OR scope_id IS NULL),
  CHECK (metric = 'requests' OR operation = 'inference'),
  CHECK (mcp_server_id IS NULL OR operation = 'mcp_tool_call'),
  CHECK (model_key IS NULL OR operation IN ('inference', 'count_tokens')),
  FOREIGN KEY (mcp_server_id) REFERENCES external_mcp_servers(mcp_server_id) ON DELETE CASCADE
);

-- Shared counters, one row per counter and fixed window. Sliding windows read
-- the previous window's row, so rows expire one window after they end.
CREATE TABLE IF NOT EXISTS rate_limit_counters (
  counter_key TEXT NOT NULL,
  window_start INTEGER NOT NULL,
  amount INTEGER NOT NULL CHECK (amount >= 0),
  expires_at INTEGER NOT NULL,
  PRIMARY KEY (counter_key, window_start)
);

CREATE INDEX IF NOT EXISTS rate_limit_counters_expiry_idx
  ON rate_limit_counters (expires_at);
