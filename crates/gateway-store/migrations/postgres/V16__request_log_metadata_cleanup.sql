UPDATE request_logs
SET metadata_json = ((metadata_json::jsonb - 'fallback_used') - 'attempt_count')::text
WHERE metadata_json::jsonb ? 'fallback_used'
   OR metadata_json::jsonb ? 'attempt_count';
