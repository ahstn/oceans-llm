CREATE TABLE IF NOT EXISTS external_mcp_servers (
  mcp_server_id TEXT PRIMARY KEY,
  server_key TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  description TEXT,
  transport TEXT NOT NULL CHECK (transport IN ('streamable_http')),
  server_url TEXT NOT NULL,
  auth_mode TEXT NOT NULL CHECK (auth_mode IN ('none', 'gateway_static_header', 'gateway_bearer_token', 'user_passthrough', 'oauth_obo')),
  auth_config_json JSONB NOT NULL DEFAULT '{}'::jsonb,
  timeout_ms BIGINT NOT NULL CHECK (timeout_ms > 0),
  status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
  last_discovery_status TEXT CHECK (last_discovery_status IN ('success', 'failed', 'auth_required', 'disabled')),
  last_discovery_at BIGINT,
  last_successful_discovery_at BIGINT,
  last_error_summary TEXT,
  last_tool_count BIGINT CHECK (last_tool_count IS NULL OR last_tool_count >= 0),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  disabled_at BIGINT
);

CREATE INDEX IF NOT EXISTS external_mcp_servers_status_idx
  ON external_mcp_servers (status, server_key);

CREATE TABLE IF NOT EXISTS external_mcp_tools (
  mcp_tool_id TEXT PRIMARY KEY,
  mcp_server_id TEXT NOT NULL,
  upstream_name TEXT NOT NULL,
  display_name TEXT NOT NULL,
  description TEXT,
  input_schema_json JSONB NOT NULL,
  schema_hash TEXT NOT NULL,
  schema_version BIGINT NOT NULL,
  is_active BIGINT NOT NULL CHECK (is_active IN (0, 1)),
  first_discovered_at BIGINT NOT NULL,
  last_discovered_at BIGINT NOT NULL,
  deactivated_at BIGINT,
  UNIQUE (mcp_server_id, upstream_name),
  FOREIGN KEY (mcp_server_id) REFERENCES external_mcp_servers(mcp_server_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS external_mcp_tools_server_active_idx
  ON external_mcp_tools (mcp_server_id, is_active, upstream_name);

CREATE INDEX IF NOT EXISTS external_mcp_tools_schema_hash_idx
  ON external_mcp_tools (schema_hash);

CREATE TABLE IF NOT EXISTS external_mcp_discovery_runs (
  discovery_run_id TEXT PRIMARY KEY,
  mcp_server_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('success', 'failed', 'auth_required', 'disabled')),
  started_at BIGINT NOT NULL,
  finished_at BIGINT NOT NULL CHECK (finished_at >= started_at),
  discovered_tool_count BIGINT NOT NULL DEFAULT 0 CHECK (discovered_tool_count >= 0),
  active_tool_count BIGINT NOT NULL DEFAULT 0 CHECK (active_tool_count >= 0),
  schema_set_hash TEXT,
  error_summary TEXT,
  details_json JSONB NOT NULL DEFAULT '{}'::jsonb,
  FOREIGN KEY (mcp_server_id) REFERENCES external_mcp_servers(mcp_server_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS external_mcp_discovery_runs_server_time_idx
  ON external_mcp_discovery_runs (mcp_server_id, started_at DESC);
