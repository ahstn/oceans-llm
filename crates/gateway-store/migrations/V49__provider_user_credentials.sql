CREATE TABLE IF NOT EXISTS provider_user_credentials (
    credential_id TEXT PRIMARY KEY,
    provider_key TEXT NOT NULL REFERENCES providers(provider_key) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    secret_ciphertext TEXT NOT NULL,
    secret_nonce TEXT NOT NULL,
    secret_key_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_used_at INTEGER,
    UNIQUE(provider_key, user_id)
);

CREATE INDEX IF NOT EXISTS idx_provider_user_credentials_user
    ON provider_user_credentials(user_id);
