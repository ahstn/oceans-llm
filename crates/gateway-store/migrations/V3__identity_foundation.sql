PRAGMA foreign_keys = OFF;

CREATE TABLE IF NOT EXISTS teams (
  team_id TEXT PRIMARY KEY,
  team_key TEXT NOT NULL UNIQUE,
  team_name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active', 'inactive')),
  model_access_mode TEXT NOT NULL DEFAULT 'all' CHECK (model_access_mode IN ('all', 'restricted')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS teams_status_idx
  ON teams (status);

CREATE TABLE IF NOT EXISTS users (
  user_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT NOT NULL,
  email_normalized TEXT NOT NULL UNIQUE,
  global_role TEXT NOT NULL DEFAULT 'user' CHECK (global_role IN ('platform_admin', 'user')),
  auth_mode TEXT NOT NULL CHECK (auth_mode IN ('password', 'oidc', 'oauth')),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'invited', 'disabled')),
  request_logging_enabled INTEGER NOT NULL DEFAULT 1 CHECK (request_logging_enabled IN (0, 1)),
  model_access_mode TEXT NOT NULL DEFAULT 'all' CHECK (model_access_mode IN ('all', 'restricted')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS users_email_normalized_uidx
  ON users (email_normalized);

CREATE TABLE IF NOT EXISTS team_memberships (
  team_id TEXT NOT NULL,
  user_id TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (team_id, user_id),
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS team_memberships_user_idx
  ON team_memberships (user_id);

CREATE TABLE IF NOT EXISTS oidc_providers (
  oidc_provider_id TEXT PRIMARY KEY,
  provider_key TEXT NOT NULL UNIQUE,
  provider_type TEXT NOT NULL CHECK (provider_type IN ('okta', 'generic_oidc')),
  issuer_url TEXT NOT NULL,
  client_id TEXT NOT NULL,
  client_secret_ref TEXT NOT NULL,
  scopes_json TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_password_auth (
  user_id TEXT PRIMARY KEY,
  password_hash TEXT NOT NULL,
  password_updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_oidc_auth (
  user_id TEXT NOT NULL,
  oidc_provider_id TEXT NOT NULL,
  subject TEXT NOT NULL,
  email_claim TEXT,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, oidc_provider_id),
  UNIQUE (oidc_provider_id, subject),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (oidc_provider_id) REFERENCES oidc_providers(oidc_provider_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_oauth_auth (
  user_id TEXT NOT NULL,
  oauth_provider TEXT NOT NULL,
  subject TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, oauth_provider),
  UNIQUE (oauth_provider, subject),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_model_allowlist (
  user_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (user_id, model_id),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS user_model_allowlist_model_idx
  ON user_model_allowlist (model_id);

CREATE TABLE IF NOT EXISTS team_model_allowlist (
  team_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (team_id, model_id),
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS team_model_allowlist_model_idx
  ON team_model_allowlist (model_id);

CREATE TABLE IF NOT EXISTS user_budgets (
  user_budget_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly')),
  amount_usd REAL NOT NULL CHECK (amount_usd >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS user_budgets_active_user_uidx
  ON user_budgets (user_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS user_budgets_user_idx
  ON user_budgets (user_id);

INSERT OR IGNORE INTO teams (
  team_id,
  team_key,
  team_name,
  status,
  model_access_mode,
  created_at,
  updated_at
) VALUES (
  '00000000-0000-0000-0000-000000000001',
  'system-legacy',
  'System Legacy',
  'active',
  'all',
  unixepoch(),
  unixepoch()
);

CREATE TABLE api_keys_v3 (
  id TEXT PRIMARY KEY,
  public_id TEXT NOT NULL UNIQUE,
  secret_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team')),
  owner_user_id TEXT,
  owner_team_id TEXT,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  revoked_at INTEGER,
  CHECK (
    (owner_kind = 'user' AND owner_user_id IS NOT NULL AND owner_team_id IS NULL) OR
    (owner_kind = 'team' AND owner_team_id IS NOT NULL AND owner_user_id IS NULL)
  ),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

INSERT INTO api_keys_v3 (
  id,
  public_id,
  secret_hash,
  name,
  status,
  owner_kind,
  owner_user_id,
  owner_team_id,
  created_at,
  last_used_at,
  revoked_at
)
SELECT
  id,
  public_id,
  secret_hash,
  name,
  status,
  'team' AS owner_kind,
  NULL AS owner_user_id,
  '00000000-0000-0000-0000-000000000001' AS owner_team_id,
  created_at,
  last_used_at,
  revoked_at
FROM api_keys;

CREATE TABLE api_key_model_grants_v3 (
  api_key_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (api_key_id, model_id),
  FOREIGN KEY (api_key_id) REFERENCES api_keys_v3(id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

INSERT INTO api_key_model_grants_v3 (api_key_id, model_id)
SELECT api_key_id, model_id
FROM api_key_model_grants;

CREATE TABLE IF NOT EXISTS audit_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  actor_api_key_id TEXT,
  action TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT,
  details_json TEXT NOT NULL
);

CREATE TABLE audit_logs_v3 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  actor_api_key_id TEXT,
  action TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT,
  details_json TEXT NOT NULL,
  FOREIGN KEY (actor_api_key_id) REFERENCES api_keys_v3(id) ON DELETE SET NULL
);

INSERT INTO audit_logs_v3 (id, ts, actor_api_key_id, action, object_type, object_id, details_json)
SELECT id, ts, actor_api_key_id, action, object_type, object_id, details_json
FROM audit_logs;

DROP TABLE api_key_model_grants;
DROP TABLE audit_logs;
DROP TABLE api_keys;

ALTER TABLE api_keys_v3 RENAME TO api_keys;
ALTER TABLE api_key_model_grants_v3 RENAME TO api_key_model_grants;
ALTER TABLE audit_logs_v3 RENAME TO audit_logs;

CREATE INDEX IF NOT EXISTS api_keys_owner_user_idx
  ON api_keys (owner_user_id);

CREATE INDEX IF NOT EXISTS api_keys_owner_team_idx
  ON api_keys (owner_team_id);

CREATE INDEX IF NOT EXISTS api_key_model_grants_model_idx
  ON api_key_model_grants (model_id);

CREATE TABLE IF NOT EXISTS usage_cost_events (
  usage_event_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_id TEXT,
  estimated_cost_usd REAL NOT NULL CHECK (estimated_cost_usd >= 0),
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS usage_cost_events_user_time_idx
  ON usage_cost_events (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_team_time_idx
  ON usage_cost_events (team_id, occurred_at);

CREATE TABLE IF NOT EXISTS request_logs (
  request_log_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_key TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  status_code INTEGER,
  latency_ms INTEGER,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  total_tokens INTEGER,
  error_code TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS request_logs_occurred_at_idx
  ON request_logs (occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_user_time_idx
  ON request_logs (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_team_time_idx
  ON request_logs (team_id, occurred_at);

PRAGMA foreign_keys = ON;
