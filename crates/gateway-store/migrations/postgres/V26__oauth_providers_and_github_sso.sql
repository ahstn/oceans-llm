CREATE TABLE IF NOT EXISTS oauth_providers (
    oauth_provider_id TEXT PRIMARY KEY,
    provider_key TEXT NOT NULL UNIQUE,
    provider_type TEXT NOT NULL CHECK (provider_type IN ('github')),
    client_id TEXT NOT NULL,
    scopes_json TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    label TEXT,
    client_secret_ref TEXT NOT NULL,
    jit_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    jit_global_role TEXT NOT NULL DEFAULT 'user',
    jit_team_key TEXT,
    jit_team_role TEXT,
    jit_request_logging_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS oauth_login_states (
    state_hash TEXT PRIMARY KEY,
    oauth_provider_id TEXT NOT NULL REFERENCES oauth_providers(oauth_provider_id),
    pkce_verifier TEXT NOT NULL,
    redirect_to TEXT NOT NULL,
    login_hint TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oauth_login_states_expires_at
    ON oauth_login_states(expires_at);

CREATE TABLE IF NOT EXISTS user_oauth_links (
    user_id UUID PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    oauth_provider_id TEXT NOT NULL REFERENCES oauth_providers(oauth_provider_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL
);

ALTER TABLE user_oauth_auth RENAME TO user_oauth_auth_legacy;

CREATE TABLE user_oauth_auth (
  user_id UUID NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  oauth_provider_id TEXT NOT NULL REFERENCES oauth_providers(oauth_provider_id) ON DELETE CASCADE,
  subject TEXT NOT NULL,
  email_claim TEXT,
  created_at TIMESTAMPTZ NOT NULL,
  PRIMARY KEY (user_id, oauth_provider_id),
  UNIQUE (oauth_provider_id, subject)
);

-- Legacy OAuth rows predate configured OAuth providers and were not reachable through
-- the product. Preserve only rows whose legacy provider string already matches a
-- seeded provider id/key; normally this copies zero rows because providers seed after
-- migrations run.
INSERT INTO user_oauth_auth (user_id, oauth_provider_id, subject, email_claim, created_at)
SELECT legacy.user_id, provider.oauth_provider_id, legacy.subject, NULL, legacy.created_at
FROM user_oauth_auth_legacy legacy
JOIN oauth_providers provider
  ON provider.oauth_provider_id = legacy.oauth_provider
  OR provider.provider_key = legacy.oauth_provider;

DROP TABLE user_oauth_auth_legacy;
