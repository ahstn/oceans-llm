ALTER TABLE agent_session_analyses
  ADD COLUMN configuration_version TEXT,
  ADD COLUMN direct_mcp_snapshot_digest TEXT;

UPDATE agent_session_analyses
SET configuration_version = COALESCE(report_json::jsonb ->> 'configuration_version', ''),
    direct_mcp_snapshot_digest = '';

ALTER TABLE agent_session_analyses
  ALTER COLUMN configuration_version SET NOT NULL,
  ALTER COLUMN direct_mcp_snapshot_digest SET NOT NULL;

DO $$
DECLARE
  constraint_name TEXT;
BEGIN
  SELECT conname
  INTO constraint_name
  FROM pg_constraint
  WHERE conrelid = 'agent_session_analyses'::regclass
    AND contype = 'u';

  IF constraint_name IS NOT NULL THEN
    EXECUTE format(
      'ALTER TABLE agent_session_analyses DROP CONSTRAINT %I',
      constraint_name
    );
  END IF;
END
$$;

ALTER TABLE agent_session_analyses
  ADD CONSTRAINT agent_session_analyses_report_identity_key UNIQUE (
    agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at,
    observation_set_id, observation_parser_version, analyzer_version, score_policy_version,
    pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size,
    cohort_snapshot_digest, direct_mcp_snapshot_digest, configuration_version
  );
