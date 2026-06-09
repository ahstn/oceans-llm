CREATE TABLE IF NOT EXISTS mcp_upstream_credential_bindings (
    credential_binding_id TEXT PRIMARY KEY,
    mcp_server_id TEXT NOT NULL REFERENCES external_mcp_servers(mcp_server_id) ON DELETE CASCADE,
    owner_scope_kind TEXT NOT NULL CHECK (owner_scope_kind IN ('user', 'team', 'service_account')),
    owner_scope_key TEXT NOT NULL,
    owner_user_id TEXT REFERENCES users(user_id) ON DELETE CASCADE,
    owner_team_id TEXT REFERENCES teams(team_id) ON DELETE CASCADE,
    owner_service_account_id TEXT REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
    material_kind TEXT NOT NULL CHECK (material_kind IN ('static_header', 'bearer_token', 'oauth_tokens')),
    header_name TEXT,
    storage_kind TEXT NOT NULL CHECK (storage_kind IN ('encrypted_blob', 'secret_ref')),
    secret_ciphertext TEXT,
    secret_nonce TEXT,
    secret_key_id TEXT,
    secret_ref TEXT,
    expires_at INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at INTEGER,
    CHECK (
        (owner_scope_kind = 'user' AND owner_user_id IS NOT NULL AND owner_team_id IS NULL AND owner_service_account_id IS NULL)
        OR (owner_scope_kind = 'team' AND owner_user_id IS NULL AND owner_team_id IS NOT NULL AND owner_service_account_id IS NULL)
        OR (owner_scope_kind = 'service_account' AND owner_user_id IS NULL AND owner_team_id IS NOT NULL AND owner_service_account_id IS NOT NULL)
    ),
    CHECK (
        (material_kind = 'static_header' AND header_name IS NOT NULL)
        OR (material_kind IN ('bearer_token', 'oauth_tokens') AND header_name IS NULL)
    ),
    CHECK (
        (storage_kind = 'encrypted_blob' AND secret_ciphertext IS NOT NULL AND secret_nonce IS NOT NULL AND secret_key_id IS NOT NULL AND secret_ref IS NULL)
        OR (storage_kind = 'secret_ref' AND secret_ciphertext IS NULL AND secret_nonce IS NULL AND secret_key_id IS NULL AND secret_ref IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_upstream_credentials_active_owner
    ON mcp_upstream_credential_bindings(mcp_server_id, owner_scope_key)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_mcp_upstream_credentials_lookup
    ON mcp_upstream_credential_bindings(mcp_server_id, owner_scope_kind, owner_scope_key, revoked_at);

CREATE INDEX IF NOT EXISTS idx_mcp_upstream_credentials_expiry
    ON mcp_upstream_credential_bindings(expires_at)
    WHERE revoked_at IS NULL AND expires_at IS NOT NULL;
