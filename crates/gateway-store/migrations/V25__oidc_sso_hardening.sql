ALTER TABLE oidc_providers ADD COLUMN label TEXT;
ALTER TABLE oidc_providers ADD COLUMN jit_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE oidc_providers ADD COLUMN jit_global_role TEXT NOT NULL DEFAULT 'user';
ALTER TABLE oidc_providers ADD COLUMN jit_team_key TEXT;
ALTER TABLE oidc_providers ADD COLUMN jit_team_role TEXT;
ALTER TABLE oidc_providers ADD COLUMN jit_request_logging_enabled INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS oidc_login_states (
    state_hash TEXT PRIMARY KEY,
    oidc_provider_id TEXT NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    redirect_to TEXT NOT NULL,
    login_hint TEXT,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (oidc_provider_id) REFERENCES oidc_providers(oidc_provider_id)
);

CREATE INDEX IF NOT EXISTS idx_oidc_login_states_expires_at
    ON oidc_login_states(expires_at);
