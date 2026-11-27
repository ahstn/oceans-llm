-- The admin request log list orders by (occurred_at DESC, request_log_id DESC). The old
-- index held only occurred_at, so rows sharing a second were sorted after the read. This
-- index covers the whole list order and keeps occurred_at as its first key, so queries
-- that used the old index use this one.
CREATE INDEX IF NOT EXISTS request_logs_occurred_at_list_idx
  ON request_logs (occurred_at, request_log_id);

DROP INDEX IF EXISTS request_logs_occurred_at_idx;
