CREATE TABLE IF NOT EXISTS mcp_aggregate_sessions (
  session_id TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  api_key_id TEXT NOT NULL,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'service_account')),
  owner_user_id TEXT,
  owner_team_id TEXT,
  owner_service_account_id TEXT,
  protocol_version TEXT NOT NULL,
  initialized BIGINT NOT NULL DEFAULT 0,
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  revoked_at BIGINT,
  CHECK (
    (owner_kind = 'user' AND owner_user_id IS NOT NULL AND owner_service_account_id IS NULL) OR
    (owner_kind = 'service_account' AND owner_service_account_id IS NOT NULL AND owner_user_id IS NULL)
  ),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS mcp_aggregate_sessions_api_key_idx
  ON mcp_aggregate_sessions (api_key_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS mcp_aggregate_sessions_expiry_idx
  ON mcp_aggregate_sessions (expires_at, revoked_at);
