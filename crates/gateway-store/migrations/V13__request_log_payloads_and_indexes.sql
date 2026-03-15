ALTER TABLE request_logs
ADD COLUMN has_payload INTEGER NOT NULL DEFAULT 0 CHECK (has_payload IN (0, 1));

ALTER TABLE request_logs
ADD COLUMN request_payload_truncated INTEGER NOT NULL DEFAULT 0
CHECK (request_payload_truncated IN (0, 1));

ALTER TABLE request_logs
ADD COLUMN response_payload_truncated INTEGER NOT NULL DEFAULT 0
CHECK (response_payload_truncated IN (0, 1));

CREATE TABLE IF NOT EXISTS request_log_payloads (
  request_log_id TEXT PRIMARY KEY,
  request_json TEXT NOT NULL,
  response_json TEXT NOT NULL,
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS request_logs_request_id_idx
  ON request_logs (request_id);

CREATE INDEX IF NOT EXISTS request_logs_api_key_id_idx
  ON request_logs (api_key_id);

CREATE INDEX IF NOT EXISTS request_logs_user_time_idx
  ON request_logs (user_id, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_team_time_idx
  ON request_logs (team_id, occurred_at);

CREATE INDEX IF NOT EXISTS request_logs_provider_time_idx
  ON request_logs (provider_key, occurred_at);
