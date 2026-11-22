ALTER TABLE agent_sessions ADD COLUMN request_count INTEGER NOT NULL DEFAULT 0 CHECK (request_count >= 0);
ALTER TABLE agent_sessions ADD COLUMN next_ordinal INTEGER NOT NULL DEFAULT 0 CHECK (next_ordinal >= 0);
ALTER TABLE agent_sessions ADD COLUMN nested_fact_count INTEGER NOT NULL DEFAULT 0 CHECK (nested_fact_count >= 0);
ALTER TABLE agent_sessions ADD COLUMN input_generation INTEGER NOT NULL DEFAULT 0 CHECK (input_generation >= 0);

UPDATE agent_sessions SET
    request_count = (SELECT COUNT(*) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id),
    next_ordinal = (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id),
    nested_fact_count = (SELECT COALESCE(SUM(json_array_length(COALESCE(json_extract(facts_json, '$.supplied_tools'), '[]')) + json_array_length(COALESCE(json_extract(facts_json, '$.supplied_skills'), '[]')) + json_array_length(COALESCE(json_extract(facts_json, '$.file_interactions'), '[]'))), 0) FROM agent_inferred_observations o WHERE o.agent_session_id = agent_sessions.agent_session_id),
    input_generation = (SELECT COUNT(*) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id) + (SELECT COUNT(*) FROM agent_inferred_observation_sets s WHERE s.agent_session_id = agent_sessions.agent_session_id);
