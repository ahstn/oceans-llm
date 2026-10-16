-- Aggregate MCP sessions belong to an API key or to an MCP client OAuth grant,
-- never both. SQLite cannot drop NOT NULL, so the table is rebuilt.

CREATE TABLE mcp_aggregate_sessions_v60 (
  session_id TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  api_key_id TEXT,
  gateway_oauth_grant_id TEXT,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'service_account')),
  owner_user_id TEXT,
  owner_team_id TEXT,
  owner_service_account_id TEXT,
  protocol_version TEXT NOT NULL,
  initialized INTEGER NOT NULL DEFAULT 0,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  revoked_at INTEGER,
  CHECK ((api_key_id IS NULL) <> (gateway_oauth_grant_id IS NULL)),
  CHECK (
    (owner_kind = 'user' AND owner_user_id IS NOT NULL AND owner_service_account_id IS NULL) OR
    (owner_kind = 'service_account' AND owner_service_account_id IS NOT NULL AND owner_user_id IS NULL)
  ),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (gateway_oauth_grant_id) REFERENCES gateway_oauth_grants(grant_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

INSERT INTO mcp_aggregate_sessions_v60 (
  session_id, token_hash, api_key_id, owner_kind, owner_user_id, owner_team_id,
  owner_service_account_id, protocol_version, initialized, expires_at, created_at, updated_at,
  revoked_at
)
SELECT
  session_id, token_hash, api_key_id, owner_kind, owner_user_id, owner_team_id,
  owner_service_account_id, protocol_version, initialized, expires_at, created_at, updated_at,
  revoked_at
FROM mcp_aggregate_sessions;

DROP TABLE mcp_aggregate_sessions;

ALTER TABLE mcp_aggregate_sessions_v60 RENAME TO mcp_aggregate_sessions;

CREATE INDEX IF NOT EXISTS mcp_aggregate_sessions_api_key_idx
  ON mcp_aggregate_sessions (api_key_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS mcp_aggregate_sessions_oauth_grant_idx
  ON mcp_aggregate_sessions (gateway_oauth_grant_id);

CREATE INDEX IF NOT EXISTS mcp_aggregate_sessions_expiry_idx
  ON mcp_aggregate_sessions (expires_at, revoked_at);
