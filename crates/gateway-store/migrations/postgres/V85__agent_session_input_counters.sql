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
    -- JSON field extraction also decodes strings, even when only counting arrays.
    -- Replace escaped NUL only in this counting projection: PostgreSQL cannot
    -- decode it to TEXT. Match only JSON escapes (an even backslash prefix):
    -- literal "\\u0000" stays unchanged, while "\\\u0000" retains its backslash.
    -- The stored facts and their array structure are untouched.
    nested_fact_count = (
      SELECT COALESCE(SUM(
        json_array_length(COALESCE(facts -> 'supplied_tools', '[]'::json))
        + json_array_length(COALESCE(facts -> 'supplied_skills', '[]'::json))
        + json_array_length(COALESCE(facts -> 'file_interactions', '[]'::json))
      ), 0)
      FROM (
        SELECT regexp_replace(
          facts_json, '(?<!\\)((?:\\\\)*)\\u0000', '\1\\ufffd', 'g'
        )::json AS facts
        FROM agent_inferred_observations o
        WHERE o.agent_session_id = agent_sessions.agent_session_id
      ) observations
    ),
    input_generation = (SELECT COUNT(*) FROM agent_session_requests r WHERE r.agent_session_id = agent_sessions.agent_session_id) + (SELECT COUNT(*) FROM agent_inferred_observation_sets s WHERE s.agent_session_id = agent_sessions.agent_session_id);
