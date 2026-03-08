ALTER TABLE request_logs ADD COLUMN upstream_model TEXT NOT NULL DEFAULT '';
ALTER TABLE request_logs ADD COLUMN stream INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN fallback_used INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 1;
ALTER TABLE request_logs ADD COLUMN payload_available INTEGER NOT NULL DEFAULT 0;

UPDATE request_logs
SET stream = COALESCE(json_extract(metadata_json, '$.stream'), 0),
    fallback_used = COALESCE(json_extract(metadata_json, '$.fallback_used'), 0),
    attempt_count = COALESCE(json_extract(metadata_json, '$.attempt_count'), 1)
WHERE metadata_json IS NOT NULL;

CREATE TABLE IF NOT EXISTS request_log_payloads (
  request_log_id TEXT PRIMARY KEY,
  request_json TEXT NOT NULL,
  response_json TEXT NOT NULL,
  request_bytes INTEGER NOT NULL,
  response_bytes INTEGER NOT NULL,
  request_truncated INTEGER NOT NULL DEFAULT 0,
  response_truncated INTEGER NOT NULL DEFAULT 0,
  request_sha256 TEXT NOT NULL,
  response_sha256 TEXT NOT NULL,
  occurred_at INTEGER NOT NULL,
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS request_logs_request_id_idx
  ON request_logs (request_id);

CREATE INDEX IF NOT EXISTS request_logs_api_key_time_idx
  ON request_logs (api_key_id, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_team_model_time_idx
  ON request_logs (team_id, model_key, occurred_at);

CREATE INDEX IF NOT EXISTS request_log_payloads_occurred_at_idx
  ON request_log_payloads (occurred_at);
