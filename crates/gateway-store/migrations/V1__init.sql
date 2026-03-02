PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS providers (
  provider_key TEXT PRIMARY KEY,
  provider_type TEXT NOT NULL,
  config_json TEXT NOT NULL,
  secrets_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS gateway_models (
  id TEXT PRIMARY KEY,
  model_key TEXT NOT NULL UNIQUE,
  description TEXT,
  tags_json TEXT NOT NULL,
  rank INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS model_routes (
  id TEXT PRIMARY KEY,
  model_id TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 100,
  weight REAL NOT NULL DEFAULT 1.0,
  enabled INTEGER NOT NULL DEFAULT 1,
  extra_headers_json TEXT NOT NULL,
  extra_body_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (model_id, provider_key, upstream_model, priority),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE,
  FOREIGN KEY (provider_key) REFERENCES providers(provider_key) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS model_routes_model_priority_idx
  ON model_routes (model_id, priority);

CREATE TABLE IF NOT EXISTS api_keys (
  id TEXT PRIMARY KEY,
  public_id TEXT NOT NULL UNIQUE,
  secret_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  revoked_at INTEGER
);

CREATE TABLE IF NOT EXISTS api_key_model_grants (
  api_key_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (api_key_id, model_id),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS api_key_model_grants_model_idx
  ON api_key_model_grants (model_id);
