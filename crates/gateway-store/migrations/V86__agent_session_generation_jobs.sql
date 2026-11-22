ALTER TABLE agent_session_analyses RENAME TO agent_session_analyses_v85;
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
  direct_mcp_snapshot_digest TEXT NOT NULL,
  analyzed_at INTEGER NOT NULL,
  report_json TEXT NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0 CHECK (stale IN (0, 1)),
  superseded_by_analysis_id TEXT,
  expires_at INTEGER NOT NULL,
  ownership_scope_key TEXT NOT NULL,
  user_id TEXT,
  service_account_id TEXT,
  configuration_version TEXT NOT NULL,
  input_generation INTEGER NOT NULL DEFAULT 0 CHECK (input_generation >= 0),
  UNIQUE (
    agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
    observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
    pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
    cohort_snapshot_digest, direct_mcp_snapshot_digest, configuration_version, input_generation
  ),
  FOREIGN KEY (superseded_by_analysis_id) REFERENCES agent_session_analyses(analysis_id) ON DELETE SET NULL,
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE,
  FOREIGN KEY (agent_session_id) REFERENCES agent_sessions(agent_session_id) ON DELETE CASCADE,
  FOREIGN KEY (observation_set_id, agent_session_id)
    REFERENCES agent_inferred_observation_sets(observation_set_id, agent_session_id) ON DELETE CASCADE
);
INSERT INTO agent_session_analyses (
  analysis_id, agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
  observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
  pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
  cohort_snapshot_digest, direct_mcp_snapshot_digest, analyzed_at, report_json, stale,
  superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id,
  configuration_version, input_generation
)
SELECT analysis_id, agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
  observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
  pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
  cohort_snapshot_digest, direct_mcp_snapshot_digest, analyzed_at, report_json, 1,
  superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id,
  configuration_version, 0
FROM agent_session_analyses_v85;
DROP TABLE agent_session_analyses_v85;
CREATE INDEX idx_agent_session_analyses_latest
  ON agent_session_analyses(agent_session_id, input_generation, stale, analyzed_at DESC, analysis_id DESC);
CREATE INDEX idx_agent_session_analyses_expiry ON agent_session_analyses(expires_at);

ALTER TABLE agent_analysis_recompute_queue ADD COLUMN request_key TEXT NOT NULL DEFAULT '';
ALTER TABLE agent_analysis_recompute_queue ADD COLUMN requested_generation INTEGER NOT NULL DEFAULT 0 CHECK (requested_generation >= 0);
ALTER TABLE agent_analysis_recompute_queue ADD COLUMN leased_generation INTEGER NOT NULL DEFAULT 0 CHECK (leased_generation >= 0);
ALTER TABLE agent_analysis_recompute_queue ADD COLUMN request_revision INTEGER NOT NULL DEFAULT 1 CHECK (request_revision > 0);
ALTER TABLE agent_analysis_recompute_queue ADD COLUMN leased_revision INTEGER NOT NULL DEFAULT 0 CHECK (leased_revision >= 0);
ALTER TABLE agent_analysis_recompute_queue ADD COLUMN lease_epoch INTEGER NOT NULL DEFAULT 0 CHECK (lease_epoch >= 0);
DELETE FROM agent_analysis_recompute_queue
WHERE queue_item_id IN (
  SELECT queue_item_id FROM (
    SELECT queue_item_id, ROW_NUMBER() OVER (
      PARTITION BY agent_session_id ORDER BY updated_at DESC, created_at DESC, queue_item_id DESC
    ) AS position FROM agent_analysis_recompute_queue
  ) ranked WHERE position > 1
);
UPDATE agent_analysis_recompute_queue
SET request_key = queue_item_id,
    requested_generation = (SELECT input_generation FROM agent_sessions s
      WHERE s.agent_session_id = agent_analysis_recompute_queue.agent_session_id),
    status = 'pending', lease_owner = NULL, lease_expires_at = NULL,
    attempts = 0, completed_at = NULL, last_error = NULL;
CREATE UNIQUE INDEX agent_analysis_queue_session_key
  ON agent_analysis_recompute_queue(agent_session_id);
