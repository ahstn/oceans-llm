-- Aggregate MCP sessions belong to an API key or to an MCP client OAuth grant,
-- never both.

ALTER TABLE mcp_aggregate_sessions ALTER COLUMN api_key_id DROP NOT NULL;

ALTER TABLE mcp_aggregate_sessions
  ADD COLUMN gateway_oauth_grant_id TEXT
  REFERENCES gateway_oauth_grants(grant_id) ON DELETE CASCADE;

ALTER TABLE mcp_aggregate_sessions
  ADD CONSTRAINT mcp_aggregate_sessions_credential_check
  CHECK ((api_key_id IS NULL) <> (gateway_oauth_grant_id IS NULL));

CREATE INDEX IF NOT EXISTS mcp_aggregate_sessions_oauth_grant_idx
  ON mcp_aggregate_sessions (gateway_oauth_grant_id);
