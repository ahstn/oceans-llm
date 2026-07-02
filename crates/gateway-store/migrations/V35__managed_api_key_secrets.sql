CREATE TABLE IF NOT EXISTS api_key_managed_credentials (
  managed_credential_id TEXT PRIMARY KEY,
  service_account_id TEXT NOT NULL,
  config_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('generated', 'configured_value')),
  auto_create INTEGER NOT NULL CHECK (auto_create IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (service_account_id, config_key),
  UNIQUE (api_key_id),
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS api_key_managed_credentials_service_account_idx
  ON api_key_managed_credentials (service_account_id);

CREATE TABLE IF NOT EXISTS api_key_secret_materials (
  api_key_id TEXT PRIMARY KEY,
  storage_kind TEXT NOT NULL CHECK (storage_kind IN ('encrypted_blob')),
  secret_ciphertext TEXT NOT NULL,
  secret_nonce TEXT NOT NULL,
  secret_key_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_retrieved_at INTEGER,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE
);
