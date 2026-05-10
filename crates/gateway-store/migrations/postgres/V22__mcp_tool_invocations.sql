CREATE TABLE IF NOT EXISTS mcp_tool_invocations (
  mcp_tool_invocation_id TEXT PRIMARY KEY,
  request_log_id TEXT,
  request_id TEXT NOT NULL,
  api_key_id TEXT,
  user_id TEXT,
  team_id TEXT,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team')),
  server_id TEXT,
  server_display_key TEXT NOT NULL,
  server_display_name TEXT NOT NULL,
  tool_id TEXT,
  tool_display_key TEXT NOT NULL,
  tool_display_name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('success', 'unauthorized', 'policy_denied', 'upstream_error', 'gateway_error', 'timeout', 'invalid_request')),
  policy_result TEXT NOT NULL CHECK (policy_result IN ('allowed', 'denied', 'not_evaluated')),
  latency_ms BIGINT,
  error_code TEXT,
  has_payload BIGINT NOT NULL DEFAULT 0 CHECK (has_payload IN (0, 1)),
  arguments_payload_truncated BIGINT NOT NULL DEFAULT 0 CHECK (arguments_payload_truncated IN (0, 1)),
  result_payload_truncated BIGINT NOT NULL DEFAULT 0 CHECK (result_payload_truncated IN (0, 1)),
  arguments_payload_redacted BIGINT NOT NULL DEFAULT 0 CHECK (arguments_payload_redacted IN (0, 1)),
  result_payload_redacted BIGINT NOT NULL DEFAULT 0 CHECK (result_payload_redacted IN (0, 1)),
  metadata_json TEXT NOT NULL DEFAULT '{}',
  occurred_at BIGINT NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_request_id_idx
  ON mcp_tool_invocations (request_id);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_request_log_idx
  ON mcp_tool_invocations (request_log_id);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_server_display_key_time_idx
  ON mcp_tool_invocations (server_display_key, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_server_display_name_time_idx
  ON mcp_tool_invocations (server_display_name, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_tool_display_key_time_idx
  ON mcp_tool_invocations (tool_display_key, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_tool_display_name_time_idx
  ON mcp_tool_invocations (tool_display_name, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_api_key_time_idx
  ON mcp_tool_invocations (api_key_id, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_user_time_idx
  ON mcp_tool_invocations (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_team_time_idx
  ON mcp_tool_invocations (team_id, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_status_time_idx
  ON mcp_tool_invocations (status, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_policy_time_idx
  ON mcp_tool_invocations (policy_result, occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_occurred_at_brin_idx
  ON mcp_tool_invocations USING BRIN (occurred_at);

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_recent_idx
  ON mcp_tool_invocations (occurred_at DESC, mcp_tool_invocation_id DESC);

CREATE TABLE IF NOT EXISTS mcp_tool_invocation_payloads (
  mcp_tool_invocation_id TEXT PRIMARY KEY,
  arguments_json JSONB NOT NULL,
  result_json JSONB NOT NULL,
  FOREIGN KEY (mcp_tool_invocation_id)
    REFERENCES mcp_tool_invocations(mcp_tool_invocation_id) ON DELETE CASCADE
);
