CREATE TABLE IF NOT EXISTS audit_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  actor_api_key_id TEXT,
  action TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT,
  details_json TEXT NOT NULL,
  FOREIGN KEY (actor_api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL
);
