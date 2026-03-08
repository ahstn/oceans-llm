CREATE TABLE IF NOT EXISTS password_invitations (
  invitation_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER,
  revoked_at INTEGER,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS password_invitations_token_hash_uidx
  ON password_invitations (token_hash);

CREATE INDEX IF NOT EXISTS password_invitations_user_idx
  ON password_invitations (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS user_oidc_links (
  user_id TEXT PRIMARY KEY,
  oidc_provider_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (oidc_provider_id) REFERENCES oidc_providers(oidc_provider_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_sessions (
  session_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  revoked_at INTEGER,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS user_sessions_token_hash_uidx
  ON user_sessions (token_hash);

CREATE INDEX IF NOT EXISTS user_sessions_user_idx
  ON user_sessions (user_id, created_at DESC);
