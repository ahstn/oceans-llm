ALTER TABLE request_logs
  ADD COLUMN user_agent_raw TEXT;

ALTER TABLE request_logs
  ADD COLUMN agent_harness_key TEXT NOT NULL DEFAULT 'unknown';

ALTER TABLE request_logs
  ADD COLUMN agent_harness_label TEXT NOT NULL DEFAULT 'Unknown';

CREATE INDEX IF NOT EXISTS request_logs_agent_harness_time_idx
  ON request_logs (agent_harness_key, occurred_at);
