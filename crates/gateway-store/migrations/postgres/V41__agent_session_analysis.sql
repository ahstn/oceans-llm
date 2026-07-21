ALTER TABLE usage_cost_events ADD COLUMN normalized_usage_json TEXT;

CREATE TABLE IF NOT EXISTS agent_sessions (
  agent_session_id TEXT PRIMARY KEY,
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
  first_seen_at BIGINT NOT NULL,
  last_seen_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (ownership_scope_key, adapter_namespace, normalized_session_hash),
  CHECK (last_seen_at >= first_seen_at),
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (actor_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_sessions_scope_seen
  ON agent_sessions(ownership_scope_key, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS agent_task_windows (
  agent_task_id TEXT PRIMARY KEY,
  agent_session_id TEXT,
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
  started_at BIGINT NOT NULL,
  ended_at BIGINT,
  input_watermark_at BIGINT NOT NULL,
  finalized_reason TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  CHECK (ended_at IS NULL OR ended_at >= started_at),
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE SET NULL,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (team_id) REFERENCES teams(team_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (actor_user_id) REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_task_windows_scope_started
  ON agent_task_windows(ownership_scope_key, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_task_windows_session_started
  ON agent_task_windows(agent_session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_task_windows_lifecycle_watermark
  ON agent_task_windows(lifecycle, input_watermark_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_task_windows_one_open_session
  ON agent_task_windows(agent_session_id, boundary_group_key)
  WHERE lifecycle = 'open' AND agent_session_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_task_windows_one_open_unscoped
  ON agent_task_windows(ownership_scope_key, harness_key, boundary_group_key)
  WHERE lifecycle = 'open' AND agent_session_id IS NULL;

CREATE TABLE IF NOT EXISTS agent_task_window_requests (
  agent_task_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_log_id TEXT,
  usage_event_id TEXT,
  ordinal BIGINT NOT NULL,
  execution_id TEXT,
  parent_execution_id TEXT,
  normalized_session_id TEXT,
  correlation_confidence TEXT NOT NULL CHECK (correlation_confidence IN ('low', 'medium', 'high')),
  limitation_codes_json TEXT NOT NULL,
  occurred_at BIGINT NOT NULL,
  completed_at BIGINT,
  terminal_success BOOLEAN,
  PRIMARY KEY (agent_task_id, request_id),
  UNIQUE (agent_task_id, ordinal),
  CHECK (ordinal >= 0),
  CHECK (completed_at IS NULL OR completed_at >= occurred_at),
  FOREIGN KEY (agent_task_id) REFERENCES agent_task_windows(agent_task_id) ON DELETE CASCADE,
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE SET NULL,
  FOREIGN KEY (usage_event_id) REFERENCES usage_cost_events(usage_event_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_task_window_requests_request
  ON agent_task_window_requests(request_id);
CREATE INDEX IF NOT EXISTS idx_agent_task_window_requests_task_time
  ON agent_task_window_requests(agent_task_id, occurred_at, ordinal);

CREATE TABLE IF NOT EXISTS agent_inferred_observation_sets (
  observation_set_id TEXT PRIMARY KEY,
  agent_task_id TEXT NOT NULL,
  parser_version TEXT NOT NULL,
  source_watermark_at BIGINT NOT NULL,
  coverage_json TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  FOREIGN KEY (agent_task_id) REFERENCES agent_task_windows(agent_task_id) ON DELETE CASCADE,
  UNIQUE (observation_set_id, agent_task_id)
);

CREATE TABLE IF NOT EXISTS agent_inferred_observations (
  observation_id TEXT PRIMARY KEY,
  observation_set_id TEXT NOT NULL,
  agent_task_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  source_request_id TEXT NOT NULL,
  evidence TEXT NOT NULL CHECK (evidence IN ('direct', 'inferred_high', 'inferred_low', 'unavailable')),
  occurred_at BIGINT NOT NULL,
  facts_json TEXT NOT NULL,
  limitation_codes_json TEXT NOT NULL,
  FOREIGN KEY (observation_set_id, agent_task_id)
    REFERENCES agent_inferred_observation_sets(observation_set_id, agent_task_id)
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_inferred_observations_set_time
  ON agent_inferred_observations(observation_set_id, occurred_at, observation_id);

CREATE INDEX IF NOT EXISTS idx_agent_inferred_observations_task_time
  ON agent_inferred_observations(agent_task_id, occurred_at, observation_id);

CREATE TABLE IF NOT EXISTS agent_task_analyses (
  analysis_id TEXT PRIMARY KEY,
  agent_task_id TEXT NOT NULL,
  report_schema_version TEXT NOT NULL,
  boundary_policy_version TEXT NOT NULL,
  input_watermark_at BIGINT NOT NULL,
  observation_set_id TEXT NOT NULL,
  observation_parser_version TEXT NOT NULL,
  analyzer_version TEXT NOT NULL,
  score_policy_version TEXT NOT NULL,
  pricing_policy_version TEXT NOT NULL,
  cohort_version TEXT NOT NULL,
  cohort_fallback_level INTEGER NOT NULL,
  cohort_sample_size BIGINT NOT NULL,
  cohort_snapshot_digest TEXT NOT NULL,
  analyzed_at BIGINT NOT NULL,
  report_json TEXT NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0 CHECK (stale IN (0, 1)),
  superseded_by_analysis_id TEXT,
  expires_at BIGINT NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  user_id TEXT,
  service_account_id TEXT,
  UNIQUE (
    agent_task_id, report_schema_version, boundary_policy_version, input_watermark_at,
    observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
    pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
    cohort_snapshot_digest
  ),
  FOREIGN KEY (superseded_by_analysis_id) REFERENCES agent_task_analyses(analysis_id) ON DELETE SET NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (agent_task_id) REFERENCES agent_task_windows(agent_task_id) ON DELETE CASCADE,
  FOREIGN KEY (observation_set_id, agent_task_id)
    REFERENCES agent_inferred_observation_sets(observation_set_id, agent_task_id)
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_task_analyses_latest
  ON agent_task_analyses(agent_task_id, stale, analyzed_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_task_analyses_expiry
  ON agent_task_analyses(expires_at);

CREATE TABLE IF NOT EXISTS agent_analysis_recompute_queue (
  queue_item_id TEXT PRIMARY KEY,
  agent_task_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  desired_versions_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'leased', 'completed', 'failed')),
  lease_owner TEXT,
  lease_expires_at BIGINT,
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL,
  last_error TEXT,
  available_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  completed_at BIGINT,
  CHECK (attempts >= 0 AND max_attempts > 0),
  FOREIGN KEY (agent_task_id) REFERENCES agent_task_windows(agent_task_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_analysis_queue_pending
  ON agent_analysis_recompute_queue(available_at, created_at)
  WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_agent_analysis_queue_leased
  ON agent_analysis_recompute_queue(lease_expires_at, created_at)
  WHERE status = 'leased';

ALTER TABLE request_logs ADD COLUMN agent_session_id TEXT;
ALTER TABLE request_logs ADD COLUMN agent_task_id TEXT;
ALTER TABLE request_logs ADD COLUMN agent_analysis_source TEXT;
ALTER TABLE request_logs ADD COLUMN agent_analysis_coverage_json TEXT;

CREATE INDEX IF NOT EXISTS idx_request_logs_agent_session
  ON request_logs(agent_session_id, occurred_at);
CREATE INDEX IF NOT EXISTS idx_request_logs_agent_task
  ON request_logs(agent_task_id, occurred_at);
