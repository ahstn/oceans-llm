-- Extend the baseline audit_logs table into the admin audit trail (docs/adr/2026-10-15-admin-audit-log.md).
-- `ts` holds unix milliseconds. Actor and target labels are snapshots taken at
-- write time, so entries stay readable after the user, key or resource is gone.
-- `details_json` keeps redacted metadata; before/after hold redacted summaries.
ALTER TABLE audit_logs ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'system'
  CHECK (actor_kind IN ('user', 'api_key', 'system'));
ALTER TABLE audit_logs ADD COLUMN actor_user_id TEXT;
ALTER TABLE audit_logs ADD COLUMN actor_label TEXT;
ALTER TABLE audit_logs ADD COLUMN target_label TEXT;
ALTER TABLE audit_logs ADD COLUMN before_json TEXT;
ALTER TABLE audit_logs ADD COLUMN after_json TEXT;
ALTER TABLE audit_logs ADD COLUMN request_ip TEXT;
ALTER TABLE audit_logs ADD COLUMN user_agent TEXT;

-- Rows written before this migration name only an API key, and stored `ts` in
-- seconds. Keep the key actor, and convert any `ts` below 10^11 (as milliseconds
-- it would be before 1973) to milliseconds, in one pass over the table.
UPDATE audit_logs
SET actor_kind = CASE WHEN actor_api_key_id IS NOT NULL THEN 'api_key' ELSE actor_kind END,
    ts = CASE WHEN ts < 100000000000 THEN ts * 1000 ELSE ts END
WHERE actor_api_key_id IS NOT NULL OR ts < 100000000000;

CREATE INDEX IF NOT EXISTS audit_logs_ts_idx
  ON audit_logs (ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_actor_user_idx
  ON audit_logs (actor_user_id, ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_target_idx
  ON audit_logs (object_type, object_id, ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_action_idx
  ON audit_logs (action, ts DESC, id DESC);
-- Combined filters: an actor's actions, and a target ID given without its type.
-- An action's prefix is its target type, so action + target type needs no index.
CREATE INDEX IF NOT EXISTS audit_logs_actor_action_idx
  ON audit_logs (actor_user_id, action, ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_object_id_idx
  ON audit_logs (object_id, ts DESC, id DESC);
-- Target type alone needs its own ordered index: the target index above puts
-- object_id between object_type and the page order.
CREATE INDEX IF NOT EXISTS audit_logs_object_type_idx
  ON audit_logs (object_type, ts DESC, id DESC);
