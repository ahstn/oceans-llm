ALTER TABLE request_logs
ADD COLUMN IF NOT EXISTS has_payload BIGINT NOT NULL DEFAULT 0
CHECK (has_payload IN (0, 1));

ALTER TABLE request_logs
ADD COLUMN IF NOT EXISTS request_payload_truncated BIGINT NOT NULL DEFAULT 0
CHECK (request_payload_truncated IN (0, 1));

ALTER TABLE request_logs
ADD COLUMN IF NOT EXISTS response_payload_truncated BIGINT NOT NULL DEFAULT 0
CHECK (response_payload_truncated IN (0, 1));

CREATE TABLE IF NOT EXISTS request_log_payloads (
  request_log_id TEXT PRIMARY KEY REFERENCES request_logs(request_log_id) ON DELETE CASCADE,
  request_json JSONB NOT NULL,
  response_json JSONB NOT NULL
);

DROP INDEX IF EXISTS request_logs_occurred_at_idx;

CREATE INDEX IF NOT EXISTS request_logs_occurred_at_brin_idx
  ON request_logs USING BRIN (occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_request_id_idx
  ON request_logs (request_id);

CREATE INDEX IF NOT EXISTS request_logs_api_key_id_idx
  ON request_logs (api_key_id);

CREATE INDEX IF NOT EXISTS request_logs_provider_time_idx
  ON request_logs (provider_key, occurred_at);
