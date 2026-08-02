ALTER TABLE agent_session_analyses RENAME TO agent_session_analyses_v41;

CREATE TABLE agent_session_analyses (
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
  configuration_version TEXT NOT NULL,
  UNIQUE (
    agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
    observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
    pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
    cohort_snapshot_digest, configuration_version
  ),
  FOREIGN KEY (superseded_by_analysis_id) REFERENCES agent_session_analyses(analysis_id) ON DELETE SET NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE CASCADE,
  FOREIGN KEY (observation_set_id, agent_session_id)
    REFERENCES agent_inferred_observation_sets(observation_set_id, agent_session_id)
    ON DELETE CASCADE
);

INSERT INTO agent_session_analyses (
  analysis_id, agent_session_id, report_schema_version, boundary_policy_version,
  input_watermark_at, observation_set_id, observation_parser_version, analyzer_version,
  score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level,
  cohort_sample_size, cohort_snapshot_digest, analyzed_at, report_json, stale,
  superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id,
  configuration_version
)
SELECT
  analysis_id, agent_session_id, report_schema_version, boundary_policy_version,
  input_watermark_at, observation_set_id, observation_parser_version, analyzer_version,
  score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level,
  cohort_sample_size, cohort_snapshot_digest, analyzed_at, report_json, stale,
  superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id,
  COALESCE(json_extract(report_json, '$.configuration_version'), '')
FROM agent_session_analyses_v41;

DROP TABLE agent_session_analyses_v41;

CREATE INDEX idx_agent_session_analyses_latest
  ON agent_session_analyses(agent_session_id, stale, analyzed_at DESC);
CREATE INDEX idx_agent_session_analyses_expiry
  ON agent_session_analyses(expires_at);
