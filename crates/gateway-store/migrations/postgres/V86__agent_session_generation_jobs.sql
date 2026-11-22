SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '15min';

ALTER TABLE agent_session_analyses
  ADD COLUMN input_generation BIGINT NOT NULL DEFAULT 0 CHECK (input_generation >= 0);
ALTER TABLE agent_session_analyses DROP CONSTRAINT agent_session_analyses_report_identity_key;
ALTER TABLE agent_session_analyses
  ADD CONSTRAINT agent_session_analyses_report_identity_key UNIQUE (
    agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
    observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
    pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
    cohort_snapshot_digest, direct_mcp_snapshot_digest, configuration_version, input_generation
  );
DROP INDEX idx_agent_session_analyses_latest;
CREATE INDEX idx_agent_session_analyses_latest
  ON agent_session_analyses(agent_session_id, input_generation, stale, analyzed_at DESC, analysis_id DESC);
-- Old timestamp-only snapshots cannot certify which committed input they contain.
UPDATE agent_session_analyses SET stale = 1;

ALTER TABLE agent_analysis_recompute_queue
  ADD COLUMN request_key TEXT NOT NULL DEFAULT '',
  ADD COLUMN requested_generation BIGINT NOT NULL DEFAULT 0 CHECK (requested_generation >= 0),
  ADD COLUMN leased_generation BIGINT NOT NULL DEFAULT 0 CHECK (leased_generation >= 0),
  ADD COLUMN request_revision BIGINT NOT NULL DEFAULT 1 CHECK (request_revision > 0),
  ADD COLUMN leased_revision BIGINT NOT NULL DEFAULT 0 CHECK (leased_revision >= 0),
  ADD COLUMN lease_epoch BIGINT NOT NULL DEFAULT 0 CHECK (lease_epoch >= 0);

-- Queue history is disposable; preserve the latest requested policy for each session.
DELETE FROM agent_analysis_recompute_queue
WHERE queue_item_id IN (
  SELECT queue_item_id FROM (
    SELECT queue_item_id, ROW_NUMBER() OVER (
      PARTITION BY agent_session_id ORDER BY updated_at DESC, created_at DESC, queue_item_id DESC
    ) AS position FROM agent_analysis_recompute_queue
  ) ranked WHERE position > 1
);
UPDATE agent_analysis_recompute_queue q
SET request_key = queue_item_id,
    requested_generation = s.input_generation,
    status = 'pending', lease_owner = NULL, lease_expires_at = NULL,
    attempts = 0, completed_at = NULL, last_error = NULL
FROM agent_sessions s WHERE s.agent_session_id = q.agent_session_id;
CREATE UNIQUE INDEX agent_analysis_queue_session_key
  ON agent_analysis_recompute_queue(agent_session_id);
