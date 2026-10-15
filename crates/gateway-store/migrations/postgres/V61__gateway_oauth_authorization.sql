-- The gateway as an OAuth authorization server for MCP clients.
-- See docs/adr/2026-10-15-mcp-client-oauth-authorization.md.

CREATE TABLE IF NOT EXISTS gateway_oauth_clients (
  client_id TEXT PRIMARY KEY,
  registration_kind TEXT NOT NULL CHECK (registration_kind IN ('dynamic')),
  client_name TEXT,
  client_uri TEXT,
  logo_uri TEXT,
  redirect_uris_json TEXT NOT NULL,
  grant_types_json TEXT NOT NULL,
  token_endpoint_auth_method TEXT NOT NULL CHECK (
    token_endpoint_auth_method IN ('none', 'client_secret_post', 'client_secret_basic')
  ),
  client_secret_hash TEXT,
  software_id TEXT,
  software_version TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  CHECK ((token_endpoint_auth_method = 'none') = (client_secret_hash IS NULL))
);

CREATE TABLE IF NOT EXISTS gateway_oauth_authorization_requests (
  request_id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  redirect_uri TEXT NOT NULL,
  -- 1 when the client sent redirect_uri; the token request must then repeat it.
  redirect_uri_supplied BIGINT NOT NULL DEFAULT 1,
  state TEXT,
  code_challenge TEXT NOT NULL,
  resource TEXT NOT NULL,
  scope TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'denied', 'consumed')),
  user_id TEXT,
  user_session_id TEXT,
  code_hash TEXT UNIQUE,
  code_expires_at BIGINT,
  grant_id TEXT,
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  decided_at BIGINT,
  consumed_at BIGINT,
  FOREIGN KEY (client_id) REFERENCES gateway_oauth_clients(client_id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (user_session_id) REFERENCES user_sessions(session_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS gateway_oauth_authorization_requests_expiry_idx
  ON gateway_oauth_authorization_requests (expires_at);

CREATE TABLE IF NOT EXISTS gateway_oauth_grants (
  grant_id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  user_session_id TEXT NOT NULL,
  resource TEXT NOT NULL,
  scope TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  revoked_at BIGINT,
  revoked_reason TEXT,
  FOREIGN KEY (client_id) REFERENCES gateway_oauth_clients(client_id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (user_session_id) REFERENCES user_sessions(session_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS gateway_oauth_grants_user_idx
  ON gateway_oauth_grants (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS gateway_oauth_grants_session_idx
  ON gateway_oauth_grants (user_session_id);

CREATE INDEX IF NOT EXISTS gateway_oauth_grants_client_idx
  ON gateway_oauth_grants (client_id);

CREATE TABLE IF NOT EXISTS gateway_oauth_tokens (
  token_hash TEXT PRIMARY KEY,
  grant_id TEXT NOT NULL,
  token_kind TEXT NOT NULL CHECK (token_kind IN ('access', 'refresh')),
  -- Access tokens may carry a narrower scope than their grant after a refresh.
  scope TEXT NOT NULL,
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  consumed_at BIGINT,
  FOREIGN KEY (grant_id) REFERENCES gateway_oauth_grants(grant_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS gateway_oauth_tokens_grant_idx
  ON gateway_oauth_tokens (grant_id);
