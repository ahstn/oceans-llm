CREATE TABLE IF NOT EXISTS mcp_toolsets (
  toolset_id TEXT PRIMARY KEY,
  toolset_key TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  description TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  disabled_at BIGINT
);

CREATE TABLE IF NOT EXISTS mcp_toolset_tools (
  toolset_id TEXT NOT NULL,
  mcp_tool_id TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (toolset_id, mcp_tool_id),
  FOREIGN KEY (toolset_id) REFERENCES mcp_toolsets(toolset_id) ON DELETE CASCADE,
  FOREIGN KEY (mcp_tool_id) REFERENCES external_mcp_tools(mcp_tool_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_mcp_toolset_tools_tool
  ON mcp_toolset_tools(mcp_tool_id);

CREATE TABLE IF NOT EXISTS mcp_tool_grants (
  grant_id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('api_key', 'user', 'team', 'service_account')),
  subject_id TEXT NOT NULL,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('tool', 'toolset')),
  target_id TEXT NOT NULL,
  is_active BIGINT NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  revoked_at BIGINT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_tool_grants_active_unique
  ON mcp_tool_grants(subject_kind, subject_id, target_kind, target_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS idx_mcp_tool_grants_subject
  ON mcp_tool_grants(subject_kind, subject_id, is_active);

CREATE INDEX IF NOT EXISTS idx_mcp_tool_grants_target
  ON mcp_tool_grants(target_kind, target_id, is_active);

CREATE TABLE IF NOT EXISTS mcp_tool_token_estimates (
  cache_key TEXT PRIMARY KEY,
  provider_family TEXT NOT NULL,
  model_or_encoding TEXT NOT NULL,
  mcp_server_id TEXT NOT NULL,
  mcp_tool_id TEXT NOT NULL,
  tool_name TEXT NOT NULL,
  schema_hash TEXT NOT NULL,
  description_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  serializer_version TEXT NOT NULL,
  estimated_tokens BIGINT NOT NULL,
  estimator_source TEXT NOT NULL CHECK (estimator_source IN ('local_tokenizer', 'conservative_fallback')),
  confidence TEXT NOT NULL CHECK (confidence IN ('high', 'low')),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  expires_at BIGINT NOT NULL,
  FOREIGN KEY (mcp_server_id) REFERENCES external_mcp_servers(mcp_server_id) ON DELETE CASCADE,
  FOREIGN KEY (mcp_tool_id) REFERENCES external_mcp_tools(mcp_tool_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_mcp_tool_token_estimates_lookup
  ON mcp_tool_token_estimates(provider_family, model_or_encoding, mcp_server_id, mcp_tool_id, schema_hash, description_hash, protocol_version, serializer_version);

CREATE INDEX IF NOT EXISTS idx_mcp_tool_token_estimates_expires
  ON mcp_tool_token_estimates(expires_at);

CREATE TABLE IF NOT EXISTS request_mcp_token_overheads (
  request_id TEXT PRIMARY KEY,
  request_log_id TEXT,
  model_key TEXT,
  provider_family TEXT NOT NULL,
  model_or_encoding TEXT NOT NULL,
  exposed_tool_count BIGINT NOT NULL,
  estimated_definition_tokens BIGINT NOT NULL,
  estimated_result_tokens BIGINT,
  estimator_source TEXT NOT NULL CHECK (estimator_source IN ('local_tokenizer', 'conservative_fallback')),
  confidence TEXT NOT NULL CHECK (confidence IN ('high', 'low')),
  cache_hit_count BIGINT NOT NULL,
  cache_miss_count BIGINT NOT NULL,
  context_window_tokens BIGINT,
  context_window_percent_bps BIGINT,
  metadata_json JSONB NOT NULL DEFAULT '{}',
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_request_mcp_token_overheads_request_log
  ON request_mcp_token_overheads(request_log_id);
