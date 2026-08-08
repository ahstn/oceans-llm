CREATE TABLE IF NOT EXISTS mcp_oauth_states (
    state_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    mcp_server_id TEXT NOT NULL REFERENCES external_mcp_servers(mcp_server_id) ON DELETE CASCADE,
    provider_key TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    redirect_to TEXT NOT NULL,
    resource TEXT NOT NULL,
    scopes_json JSONB NOT NULL,
    expires_at BIGINT NOT NULL,
    consumed_at BIGINT,
    created_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_oauth_states_expiry
    ON mcp_oauth_states(expires_at);

CREATE TABLE IF NOT EXISTS mcp_oauth_refresh_leases (
    credential_binding_id TEXT PRIMARY KEY REFERENCES mcp_upstream_credential_bindings(credential_binding_id) ON DELETE CASCADE,
    lease_token TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_oauth_refresh_leases_expiry
    ON mcp_oauth_refresh_leases(expires_at);
