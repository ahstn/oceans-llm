UPDATE request_logs
SET metadata_json = json_remove(metadata_json, '$.fallback_used', '$.attempt_count')
WHERE json_valid(metadata_json)
  AND (
    json_type(metadata_json, '$.fallback_used') IS NOT NULL
    OR json_type(metadata_json, '$.attempt_count') IS NOT NULL
  );
