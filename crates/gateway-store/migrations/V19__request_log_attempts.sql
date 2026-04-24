CREATE TABLE IF NOT EXISTS request_log_attempts (
  request_attempt_id TEXT PRIMARY KEY,
  request_log_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  attempt_number INTEGER NOT NULL CHECK (attempt_number >= 1),
  route_id TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('success', 'provider_error', 'stream_start_error', 'stream_error')),
  status_code INTEGER,
  error_code TEXT,
  error_detail TEXT,
  error_detail_truncated INTEGER NOT NULL DEFAULT 0 CHECK (error_detail_truncated IN (0, 1)),
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
  terminal INTEGER NOT NULL DEFAULT 1 CHECK (terminal IN (0, 1)),
  produced_final_response INTEGER NOT NULL DEFAULT 0 CHECK (produced_final_response IN (0, 1)),
  stream INTEGER NOT NULL DEFAULT 0 CHECK (stream IN (0, 1)),
  started_at INTEGER NOT NULL,
  completed_at INTEGER,
  latency_ms INTEGER,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  FOREIGN KEY (request_log_id) REFERENCES request_logs(request_log_id) ON DELETE CASCADE,
  UNIQUE (request_log_id, attempt_number)
);

CREATE INDEX IF NOT EXISTS request_log_attempts_request_log_idx
  ON request_log_attempts (request_log_id, attempt_number);

CREATE INDEX IF NOT EXISTS request_log_attempts_request_id_idx
  ON request_log_attempts (request_id);
