ALTER TABLE request_logs
ADD COLUMN caller_service TEXT;

ALTER TABLE request_logs
ADD COLUMN caller_component TEXT;

ALTER TABLE request_logs
ADD COLUMN caller_env TEXT;

CREATE TABLE request_log_tags (
  request_log_id TEXT NOT NULL REFERENCES request_logs(request_log_id) ON DELETE CASCADE,
  tag_key TEXT NOT NULL,
  tag_value TEXT NOT NULL,
  PRIMARY KEY (request_log_id, tag_key)
);

CREATE INDEX request_logs_caller_service_time_idx
  ON request_logs (caller_service, occurred_at);

CREATE INDEX request_logs_caller_component_time_idx
  ON request_logs (caller_component, occurred_at);

CREATE INDEX request_logs_caller_env_time_idx
  ON request_logs (caller_env, occurred_at);

CREATE INDEX request_log_tags_lookup_idx
  ON request_log_tags (tag_key, tag_value, request_log_id);
