-- OAuth clients that Oceans registered with upstream MCP authorization servers
-- through RFC 7591 dynamic client registration. One client per MCP server.
CREATE TABLE IF NOT EXISTS mcp_oauth_clients (
    mcp_server_id TEXT PRIMARY KEY REFERENCES external_mcp_servers(mcp_server_id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    client_id TEXT NOT NULL,
    token_endpoint_auth_method TEXT NOT NULL CHECK (token_endpoint_auth_method IN ('client_secret_basic', 'client_secret_post', 'none')),
    secret_ciphertext TEXT,
    secret_nonce TEXT,
    secret_key_id TEXT,
    client_secret_expires_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK (
        (secret_ciphertext IS NULL AND secret_nonce IS NULL AND secret_key_id IS NULL)
        OR (secret_ciphertext IS NOT NULL AND secret_nonce IS NOT NULL AND secret_key_id IS NOT NULL)
    )
);
