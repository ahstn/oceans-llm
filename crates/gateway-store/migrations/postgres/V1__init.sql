CREATE TABLE IF NOT EXISTS providers (
  provider_key TEXT PRIMARY KEY,
  provider_type TEXT NOT NULL,
  config_json TEXT NOT NULL,
  secrets_json TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS gateway_models (
  id TEXT PRIMARY KEY,
  model_key TEXT NOT NULL UNIQUE,
  description TEXT,
  tags_json TEXT NOT NULL,
  rank INTEGER NOT NULL DEFAULT 100,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_routes (
  id TEXT PRIMARY KEY,
  model_id TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 100,
  weight DOUBLE PRECISION NOT NULL DEFAULT 1.0,
  enabled BIGINT NOT NULL DEFAULT 1,
  extra_headers_json TEXT NOT NULL,
  extra_body_json TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (model_id, provider_key, upstream_model, priority),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE,
  FOREIGN KEY (provider_key) REFERENCES providers(provider_key) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS model_routes_model_priority_idx
  ON model_routes (model_id, priority);

CREATE TABLE IF NOT EXISTS teams (
  team_id TEXT PRIMARY KEY,
  team_key TEXT NOT NULL UNIQUE,
  team_name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active', 'inactive')),
  model_access_mode TEXT NOT NULL DEFAULT 'all' CHECK (model_access_mode IN ('all', 'restricted')),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
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
  must_change_password BIGINT NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
  request_logging_enabled BIGINT NOT NULL DEFAULT 1 CHECK (request_logging_enabled IN (0, 1)),
  model_access_mode TEXT NOT NULL DEFAULT 'all' CHECK (model_access_mode IN ('all', 'restricted')),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS users_email_normalized_uidx
  ON users (email_normalized);

CREATE TABLE IF NOT EXISTS team_memberships (
  team_id TEXT NOT NULL,
  user_id TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
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
  enabled BIGINT NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_password_auth (
  user_id TEXT PRIMARY KEY,
  password_hash TEXT NOT NULL,
  password_updated_at BIGINT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_oidc_auth (
  user_id TEXT NOT NULL,
  oidc_provider_id TEXT NOT NULL,
  subject TEXT NOT NULL,
  email_claim TEXT,
  created_at BIGINT NOT NULL,
  PRIMARY KEY (user_id, oidc_provider_id),
  UNIQUE (oidc_provider_id, subject),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (oidc_provider_id) REFERENCES oidc_providers(oidc_provider_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_oauth_auth (
  user_id TEXT NOT NULL,
  oauth_provider TEXT NOT NULL,
  subject TEXT NOT NULL,
  created_at BIGINT NOT NULL,
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

CREATE TABLE IF NOT EXISTS api_keys (
  id TEXT PRIMARY KEY,
  public_id TEXT NOT NULL UNIQUE,
  secret_hash TEXT NOT NULL,
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team')),
  owner_user_id TEXT,
  owner_team_id TEXT,
  created_at BIGINT NOT NULL,
  last_used_at BIGINT,
  revoked_at BIGINT,
  CHECK (
    (owner_kind = 'user' AND owner_user_id IS NOT NULL AND owner_team_id IS NULL) OR
    (owner_kind = 'team' AND owner_team_id IS NOT NULL AND owner_user_id IS NULL)
  ),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS api_keys_owner_user_idx
  ON api_keys (owner_user_id);

CREATE INDEX IF NOT EXISTS api_keys_owner_team_idx
  ON api_keys (owner_team_id);

CREATE TABLE IF NOT EXISTS api_key_model_grants (
  api_key_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  PRIMARY KEY (api_key_id, model_id),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS api_key_model_grants_model_idx
  ON api_key_model_grants (model_id);

CREATE TABLE IF NOT EXISTS audit_logs (
  id BIGSERIAL PRIMARY KEY,
  ts BIGINT NOT NULL,
  actor_api_key_id TEXT,
  action TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT,
  details_json TEXT NOT NULL,
  FOREIGN KEY (actor_api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS user_budgets (
  user_budget_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly')),
  amount_10000 BIGINT NOT NULL CHECK (amount_10000 >= 0),
  hard_limit BIGINT NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active BIGINT NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS user_budgets_active_user_uidx
  ON user_budgets (user_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS user_budgets_user_idx
  ON user_budgets (user_id);

CREATE TABLE IF NOT EXISTS usage_cost_events (
  usage_event_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_id TEXT,
  estimated_cost_10000 BIGINT NOT NULL CHECK (estimated_cost_10000 >= 0),
  occurred_at BIGINT NOT NULL,
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
  status_code BIGINT,
  latency_ms BIGINT,
  prompt_tokens BIGINT,
  completion_tokens BIGINT,
  total_tokens BIGINT,
  error_code TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  occurred_at BIGINT NOT NULL,
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

CREATE TABLE IF NOT EXISTS pricing_catalog_cache (
  catalog_key TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  etag TEXT,
  fetched_at BIGINT NOT NULL,
  snapshot_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS password_invitations (
  invitation_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  expires_at BIGINT NOT NULL,
  consumed_at BIGINT,
  revoked_at BIGINT,
  created_at BIGINT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS password_invitations_token_hash_uidx
  ON password_invitations (token_hash);

CREATE INDEX IF NOT EXISTS password_invitations_user_idx
  ON password_invitations (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS user_oidc_links (
  user_id TEXT PRIMARY KEY,
  oidc_provider_id TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (oidc_provider_id) REFERENCES oidc_providers(oidc_provider_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_sessions (
  session_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  last_seen_at BIGINT NOT NULL,
  revoked_at BIGINT,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS user_sessions_token_hash_uidx
  ON user_sessions (token_hash);

CREATE INDEX IF NOT EXISTS user_sessions_user_idx
  ON user_sessions (user_id, created_at DESC);
