-- Counters are maintained by V85 writers. Older writers must not run after this backfill.
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '15min';

ALTER TABLE agent_sessions ADD COLUMN request_count BIGINT NOT NULL DEFAULT 0 CHECK (request_count >= 0);
ALTER TABLE agent_sessions ADD COLUMN next_ordinal BIGINT NOT NULL DEFAULT 0 CHECK (next_ordinal >= 0);
ALTER TABLE agent_sessions ADD COLUMN nested_fact_count BIGINT NOT NULL DEFAULT 0 CHECK (nested_fact_count >= 0);
ALTER TABLE agent_sessions ADD COLUMN input_generation BIGINT NOT NULL DEFAULT 0 CHECK (input_generation >= 0);

UPDATE agent_sessions SET
    request_count = (SELECT COUNT(*) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id),
    next_ordinal = (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id),
    nested_fact_count = (SELECT COALESCE(SUM(jsonb_array_length(COALESCE(facts_json::jsonb -> 'supplied_tools', '[]'::jsonb)) + jsonb_array_length(COALESCE(facts_json::jsonb -> 'supplied_skills', '[]'::jsonb)) + jsonb_array_length(COALESCE(facts_json::jsonb -> 'file_interactions', '[]'::jsonb))), 0) FROM agent_inferred_observations o WHERE o.agent_session_id = agent_sessions.agent_session_id),
    input_generation = (SELECT COUNT(*) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id) + (SELECT COUNT(*) FROM agent_inferred_observation_sets s WHERE s.agent_session_id = agent_sessions.agent_session_id);
