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

CREATE INDEX IF NOT EXISTS audit_logs_ts_idx
  ON audit_logs (ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_actor_user_idx
  ON audit_logs (actor_user_id, ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_target_idx
  ON audit_logs (object_type, object_id, ts DESC, id DESC);
CREATE INDEX IF NOT EXISTS audit_logs_action_idx
  ON audit_logs (action, ts DESC, id DESC);
