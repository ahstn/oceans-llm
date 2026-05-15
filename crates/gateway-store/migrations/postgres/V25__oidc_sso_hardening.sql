ALTER TABLE oidc_providers ADD COLUMN IF NOT EXISTS label TEXT;
ALTER TABLE oidc_providers ADD COLUMN IF NOT EXISTS jit_enabled BIGINT NOT NULL DEFAULT 0;
ALTER TABLE oidc_providers ADD COLUMN IF NOT EXISTS jit_global_role TEXT NOT NULL DEFAULT 'user';
ALTER TABLE oidc_providers ADD COLUMN IF NOT EXISTS jit_team_key TEXT;
ALTER TABLE oidc_providers ADD COLUMN IF NOT EXISTS jit_team_role TEXT;
ALTER TABLE oidc_providers ADD COLUMN IF NOT EXISTS jit_request_logging_enabled BIGINT NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS oidc_login_states (
    state_hash TEXT PRIMARY KEY,
    oidc_provider_id TEXT NOT NULL REFERENCES oidc_providers(oidc_provider_id),
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    redirect_to TEXT NOT NULL,
    login_hint TEXT,
    expires_at BIGINT NOT NULL,
    consumed_at BIGINT,
    created_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oidc_login_states_expires_at
    ON oidc_login_states(expires_at);
