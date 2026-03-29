CREATE OR REPLACE FUNCTION pg_temp.try_parse_jsonb(raw text)
RETURNS jsonb
LANGUAGE plpgsql
AS $$
BEGIN
  RETURN raw::jsonb;
EXCEPTION WHEN others THEN
  RETURN jsonb_build_object('__oceans_parse_failed__', true);
END;
$$;

WITH parsed AS (
  SELECT
    request_log_id,
    pg_temp.try_parse_jsonb(metadata_json) AS metadata
  FROM request_logs
)
UPDATE request_logs AS request_logs
SET metadata_json = ((parsed.metadata - 'fallback_used') - 'attempt_count')::text
FROM parsed
WHERE request_logs.request_log_id = parsed.request_log_id
  AND parsed.metadata->>'__oceans_parse_failed__' IS DISTINCT FROM 'true'
  AND (
    parsed.metadata ? 'fallback_used'
    OR parsed.metadata ? 'attempt_count'
  );
