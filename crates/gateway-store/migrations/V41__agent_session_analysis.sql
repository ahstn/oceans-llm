ALTER TABLE usage_cost_events ADD COLUMN normalized_usage_json TEXT;

CREATE TABLE IF NOT EXISTS agent_session_sources (
  agent_session_source_id TEXT PRIMARY KEY,
  ownership_scope_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  service_account_id TEXT,
  actor_user_id TEXT,
  normalized_session_hash TEXT NOT NULL,
  adapter_namespace TEXT NOT NULL,
  adapter_version TEXT NOT NULL,
  source_provenance TEXT NOT NULL,
  harness_key TEXT NOT NULL,
  harness_label TEXT NOT NULL,
  first_seen_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (ownership_scope_key, adapter_namespace, normalized_session_hash),
  CHECK (last_seen_at >= first_seen_at),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (actor_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_session_sources_scope_seen
  ON agent_session_sources(ownership_scope_key, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS agent_sessions (
  agent_session_id TEXT PRIMARY KEY,
  agent_session_source_id TEXT,
  ownership_scope_key TEXT NOT NULL,
  api_key_id TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  service_account_id TEXT,
  actor_user_id TEXT,
  requested_model_key TEXT NOT NULL,
  operation TEXT NOT NULL,
  caller_class TEXT NOT NULL,
  request_tags_json TEXT NOT NULL,
  harness_key TEXT NOT NULL,
  boundary_group_key TEXT NOT NULL,
  boundary_policy_version TEXT NOT NULL,
  lifecycle TEXT NOT NULL CHECK (lifecycle IN ('open', 'finalized')),
  boundary_confidence TEXT NOT NULL CHECK (boundary_confidence IN ('low', 'medium', 'high')),
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  input_watermark_at INTEGER NOT NULL,
  finalized_reason TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (ended_at IS NULL OR ended_at >= started_at),
  FOREIGN KEY (agent_session_source_id) REFERENCES agent_session_sources(agent_session_source_id) ON DELETE SET NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (actor_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_scope_started
  ON agent_sessions(ownership_scope_key, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_sessions_source_started
  ON agent_sessions(agent_session_source_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_sessions_lifecycle_watermark
  ON agent_sessions(lifecycle, input_watermark_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_sessions_one_open_source
  ON agent_sessions(agent_session_source_id, boundary_group_key)
  WHERE lifecycle = 'open' AND agent_session_source_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_sessions_one_open_unscoped
  ON agent_sessions(ownership_scope_key, harness_key, boundary_group_key)
  WHERE lifecycle = 'open' AND agent_session_source_id IS NULL;

CREATE TABLE IF NOT EXISTS agent_session_requests (
  agent_session_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_log_id TEXT,
  usage_event_id TEXT,
  ordinal INTEGER NOT NULL,
  execution_id TEXT,
  parent_execution_id TEXT,
  normalized_session_id TEXT,
  correlation_confidence TEXT NOT NULL CHECK (correlation_confidence IN ('low', 'medium', 'high')),
  limitation_codes_json TEXT NOT NULL,
  occurred_at INTEGER NOT NULL,
  completed_at INTEGER,
  terminal_success INTEGER CHECK (terminal_success IN (0, 1)),
  PRIMARY KEY (agent_session_id, request_id),
  UNIQUE (agent_session_id, ordinal),
  CHECK (ordinal >= 0),
  CHECK (completed_at IS NULL OR completed_at >= occurred_at),
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE CASCADE,
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE SET NULL,
  FOREIGN KEY (usage_event_id) REFERENCES usage_cost_events(usage_event_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_session_requests_request
  ON agent_session_requests(request_id);
CREATE INDEX IF NOT EXISTS idx_agent_session_requests_session_time
  ON agent_session_requests(agent_session_id, occurred_at, ordinal);

CREATE TABLE IF NOT EXISTS agent_inferred_observation_sets (
  observation_set_id TEXT PRIMARY KEY,
  agent_session_id TEXT NOT NULL,
  parser_version TEXT NOT NULL,
  source_watermark_at INTEGER NOT NULL,
  coverage_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE CASCADE,
  UNIQUE (observation_set_id, agent_session_id)
);

CREATE TABLE IF NOT EXISTS agent_inferred_observations (
  observation_id TEXT PRIMARY KEY,
  observation_set_id TEXT NOT NULL,
  agent_session_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  source_request_id TEXT NOT NULL,
  evidence TEXT NOT NULL CHECK (evidence IN ('direct', 'inferred_high', 'inferred_low', 'unavailable')),
  occurred_at INTEGER NOT NULL,
  facts_json TEXT NOT NULL,
  limitation_codes_json TEXT NOT NULL,
  FOREIGN KEY (observation_set_id, agent_session_id)
    REFERENCES agent_inferred_observation_sets(observation_set_id, agent_session_id)
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_inferred_observations_set_time
  ON agent_inferred_observations(observation_set_id, occurred_at, observation_id);

CREATE INDEX IF NOT EXISTS idx_agent_inferred_observations_session_time
  ON agent_inferred_observations(agent_session_id, occurred_at, observation_id);

CREATE TABLE IF NOT EXISTS agent_session_analyses (
  analysis_id TEXT PRIMARY KEY,
  agent_session_id TEXT NOT NULL,
  report_schema_version TEXT NOT NULL,
  boundary_policy_version TEXT NOT NULL,
  input_watermark_at INTEGER NOT NULL,
  observation_set_id TEXT NOT NULL,
  observation_parser_version TEXT NOT NULL,
  analyzer_version TEXT NOT NULL,
  score_policy_version TEXT NOT NULL,
  pricing_policy_version TEXT NOT NULL,
  cohort_version TEXT NOT NULL,
  cohort_fallback_level INTEGER NOT NULL,
  cohort_sample_size INTEGER NOT NULL,
  cohort_snapshot_digest TEXT NOT NULL,
  analyzed_at INTEGER NOT NULL,
  report_json TEXT NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0 CHECK (stale IN (0, 1)),
  superseded_by_analysis_id TEXT,
  expires_at INTEGER NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  user_id TEXT,
  service_account_id TEXT,
  UNIQUE (
    agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
    observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
    pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
    cohort_snapshot_digest
  ),
  FOREIGN KEY (superseded_by_analysis_id) REFERENCES agent_session_analyses(analysis_id) ON DELETE SET NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE CASCADE,
  FOREIGN KEY (observation_set_id, agent_session_id)
    REFERENCES agent_inferred_observation_sets(observation_set_id, agent_session_id)
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_session_analyses_latest
  ON agent_session_analyses(agent_session_id, stale, analyzed_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_session_analyses_expiry
  ON agent_session_analyses(expires_at);

CREATE TABLE IF NOT EXISTS agent_analysis_recompute_queue (
  queue_item_id TEXT PRIMARY KEY,
  agent_session_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  desired_versions_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'leased', 'completed', 'failed')),
  lease_owner TEXT,
  lease_expires_at INTEGER,
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL,
  last_error TEXT,
  available_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  completed_at INTEGER,
  CHECK (attempts >= 0 AND max_attempts > 0),
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_analysis_queue_pending
  ON agent_analysis_recompute_queue(available_at, created_at)
  WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_agent_analysis_queue_leased
  ON agent_analysis_recompute_queue(lease_expires_at, created_at)
  WHERE status = 'leased';

ALTER TABLE request_logs ADD COLUMN agent_session_source_id TEXT;
ALTER TABLE request_logs ADD COLUMN agent_session_id TEXT;
ALTER TABLE request_logs ADD COLUMN agent_analysis_source TEXT;
ALTER TABLE request_logs ADD COLUMN agent_analysis_coverage_json TEXT;

CREATE INDEX IF NOT EXISTS idx_request_logs_agent_session_source
  ON request_logs(agent_session_source_id, occurred_at);
CREATE INDEX IF NOT EXISTS idx_request_logs_agent_session
  ON request_logs(agent_session_id, occurred_at);
