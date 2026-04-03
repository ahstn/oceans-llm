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
  alias_target_model_id TEXT REFERENCES gateway_models(id) ON DELETE SET NULL,
  description TEXT,
  tags_json TEXT NOT NULL,
  rank INTEGER NOT NULL DEFAULT 100,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS gateway_models_alias_target_idx
  ON gateway_models (alias_target_model_id);

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
  must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
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

CREATE TABLE IF NOT EXISTS api_keys (
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
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  actor_api_key_id TEXT,
  action TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT,
  details_json TEXT NOT NULL,
  FOREIGN KEY (actor_api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL
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
  capabilities_json TEXT NOT NULL DEFAULT '{"chat_completions":true,"stream":true,"embeddings":true,"tools":true,"vision":true,"json_schema":true,"developer_role":true}',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (model_id, provider_key, upstream_model, priority),
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE CASCADE,
  FOREIGN KEY (provider_key) REFERENCES providers(provider_key) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS model_routes_model_priority_idx
  ON model_routes (model_id, priority);

CREATE TABLE IF NOT EXISTS pricing_catalog_cache (
  catalog_key TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  etag TEXT,
  fetched_at INTEGER NOT NULL,
  snapshot_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS password_invitations (
  invitation_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER,
  revoked_at INTEGER,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS password_invitations_token_hash_uidx
  ON password_invitations (token_hash);

CREATE INDEX IF NOT EXISTS password_invitations_user_idx
  ON password_invitations (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS user_oidc_links (
  user_id TEXT PRIMARY KEY,
  oidc_provider_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (oidc_provider_id) REFERENCES oidc_providers(oidc_provider_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_sessions (
  session_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  revoked_at INTEGER,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS user_sessions_token_hash_uidx
  ON user_sessions (token_hash);

CREATE INDEX IF NOT EXISTS user_sessions_user_idx
  ON user_sessions (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS model_pricing (
  model_pricing_id TEXT PRIMARY KEY,
  pricing_provider_id TEXT NOT NULL,
  pricing_model_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  input_cost_per_million_tokens_10000 INTEGER,
  output_cost_per_million_tokens_10000 INTEGER,
  cache_read_cost_per_million_tokens_10000 INTEGER,
  cache_write_cost_per_million_tokens_10000 INTEGER,
  input_audio_cost_per_million_tokens_10000 INTEGER,
  output_audio_cost_per_million_tokens_10000 INTEGER,
  release_date TEXT NOT NULL,
  last_updated TEXT NOT NULL,
  effective_start_at INTEGER NOT NULL,
  effective_end_at INTEGER,
  limits_json TEXT NOT NULL DEFAULT '{}',
  modalities_json TEXT NOT NULL DEFAULT '{}',
  provenance_source TEXT NOT NULL,
  provenance_etag TEXT,
  provenance_fetched_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS model_pricing_provider_model_start_uidx
  ON model_pricing (pricing_provider_id, pricing_model_id, effective_start_at);

CREATE UNIQUE INDEX IF NOT EXISTS model_pricing_active_uidx
  ON model_pricing (pricing_provider_id, pricing_model_id)
  WHERE effective_end_at IS NULL;

CREATE INDEX IF NOT EXISTS model_pricing_lookup_idx
  ON model_pricing (pricing_provider_id, pricing_model_id, effective_start_at, effective_end_at);

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
  resolved_model_key TEXT,
  has_payload INTEGER NOT NULL DEFAULT 0 CHECK (has_payload IN (0, 1)),
  request_payload_truncated INTEGER NOT NULL DEFAULT 0 CHECK (request_payload_truncated IN (0, 1)),
  response_payload_truncated INTEGER NOT NULL DEFAULT 0 CHECK (response_payload_truncated IN (0, 1)),
  caller_service TEXT,
  caller_component TEXT,
  caller_env TEXT,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS request_logs_occurred_at_idx
  ON request_logs (occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_request_id_idx
  ON request_logs (request_id);

CREATE INDEX IF NOT EXISTS request_logs_api_key_id_idx
  ON request_logs (api_key_id);

CREATE INDEX IF NOT EXISTS request_logs_user_time_idx
  ON request_logs (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_team_time_idx
  ON request_logs (team_id, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_provider_time_idx
  ON request_logs (provider_key, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_caller_service_time_idx
  ON request_logs (caller_service, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_caller_component_time_idx
  ON request_logs (caller_component, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_caller_env_time_idx
  ON request_logs (caller_env, occurred_at);

CREATE TABLE IF NOT EXISTS request_log_payloads (
  request_log_id TEXT PRIMARY KEY,
  request_json TEXT NOT NULL,
  response_json TEXT NOT NULL,
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS request_log_tags (
  request_log_id TEXT NOT NULL,
  tag_key TEXT NOT NULL,
  tag_value TEXT NOT NULL,
  PRIMARY KEY (request_log_id, tag_key),
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS request_log_tags_lookup_idx
  ON request_log_tags (tag_key, tag_value, request_log_id);

CREATE TABLE IF NOT EXISTS user_budgets (
  user_budget_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
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

CREATE TABLE IF NOT EXISTS team_budgets (
  team_budget_id TEXT PRIMARY KEY,
  team_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  amount_10000 INTEGER NOT NULL CHECK (amount_10000 >= 0),
  hard_limit INTEGER NOT NULL DEFAULT 1 CHECK (hard_limit IN (0, 1)),
  timezone TEXT NOT NULL DEFAULT 'UTC',
  is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS team_budgets_active_team_uidx
  ON team_budgets (team_id)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS team_budgets_team_idx
  ON team_budgets (team_id);

CREATE TABLE IF NOT EXISTS budget_alerts (
  budget_alert_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'team')),
  owner_id TEXT NOT NULL,
  owner_name TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  cadence TEXT NOT NULL CHECK (cadence IN ('daily', 'weekly', 'monthly')),
  threshold_bps INTEGER NOT NULL CHECK (threshold_bps > 0 AND threshold_bps <= 10000),
  window_start INTEGER NOT NULL,
  window_end INTEGER NOT NULL,
  spend_before_10000 INTEGER NOT NULL CHECK (spend_before_10000 >= 0),
  spend_after_10000 INTEGER NOT NULL CHECK (spend_after_10000 >= 0),
  remaining_budget_10000 INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alerts_scope_threshold_window_uidx
  ON budget_alerts (ownership_scope_key, budget_id, threshold_bps, window_start);

CREATE INDEX IF NOT EXISTS budget_alerts_created_at_idx
  ON budget_alerts (created_at DESC);

CREATE TABLE IF NOT EXISTS budget_alert_deliveries (
  budget_alert_delivery_id TEXT PRIMARY KEY,
  budget_alert_id TEXT NOT NULL,
  channel TEXT NOT NULL CHECK (channel IN ('email')),
  delivery_status TEXT NOT NULL CHECK (delivery_status IN ('pending', 'sent', 'failed')),
  recipient TEXT,
  provider_message_id TEXT,
  failure_reason TEXT,
  queued_at INTEGER NOT NULL,
  last_attempted_at INTEGER,
  sent_at INTEGER,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (budget_alert_id) REFERENCES budget_alerts(budget_alert_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS budget_alert_deliveries_alert_recipient_uidx
  ON budget_alert_deliveries (budget_alert_id, channel, recipient);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_pending_idx
  ON budget_alert_deliveries (delivery_status, queued_at);

CREATE INDEX IF NOT EXISTS budget_alert_deliveries_alert_idx
  ON budget_alert_deliveries (budget_alert_id);

CREATE TABLE IF NOT EXISTS usage_cost_event_duplicates_archive (
  archived_duplicate_id TEXT PRIMARY KEY,
  original_usage_event_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  model_id TEXT,
  original_estimated_cost_10000 INTEGER NOT NULL CHECK (original_estimated_cost_10000 >= 0),
  occurred_at INTEGER NOT NULL,
  archived_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS usage_cost_events (
  usage_event_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  actor_user_id TEXT,
  model_id TEXT,
  provider_key TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  total_tokens INTEGER,
  provider_usage_json TEXT NOT NULL DEFAULT '{}',
  pricing_status TEXT NOT NULL CHECK (
    pricing_status IN ('priced', 'unpriced', 'usage_missing', 'legacy_estimated')
  ),
  unpriced_reason TEXT,
  pricing_row_id TEXT,
  pricing_provider_id TEXT,
  pricing_model_id TEXT,
  pricing_source TEXT,
  pricing_source_etag TEXT,
  pricing_source_fetched_at INTEGER,
  pricing_last_updated TEXT,
  input_cost_per_million_tokens_10000 INTEGER,
  output_cost_per_million_tokens_10000 INTEGER,
  computed_cost_10000 INTEGER NOT NULL CHECK (computed_cost_10000 >= 0),
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE SET NULL,
  FOREIGN KEY (actor_user_id) REFERENCES users(user_id) ON DELETE SET NULL,
  FOREIGN KEY (model_id) REFERENCES gateway_models(id) ON DELETE SET NULL,
  FOREIGN KEY (pricing_row_id) REFERENCES model_pricing(model_pricing_id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS usage_cost_events_request_scope_uidx
  ON usage_cost_events (request_id, ownership_scope_key);

CREATE INDEX IF NOT EXISTS usage_cost_events_user_time_idx
  ON usage_cost_events (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_team_time_idx
  ON usage_cost_events (team_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_pricing_status_idx
  ON usage_cost_events (pricing_status, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_occurred_at_idx
  ON usage_cost_events (occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_model_time_idx
  ON usage_cost_events (model_id, occurred_at);

CREATE INDEX IF NOT EXISTS usage_cost_events_upstream_model_time_idx
  ON usage_cost_events (upstream_model, occurred_at);
