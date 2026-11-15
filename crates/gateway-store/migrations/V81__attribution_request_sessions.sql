-- A journal can be reported before the gateway finishes persisting the request.
-- Only the owner-scoped canonical Oceans session hash is stored, never the journal id.
CREATE TABLE attribution_request_sessions (
  ownership_scope_key TEXT NOT NULL,
  owner_user_id TEXT,
  owner_service_account_id TEXT,
  request_id TEXT NOT NULL CHECK (length(request_id) = 36),
  session_hash TEXT NOT NULL,
  installation_id TEXT NOT NULL,
  sequence INTEGER NOT NULL CHECK (sequence >= 0),
  observed_at INTEGER NOT NULL,
  late_sequenced INTEGER NOT NULL CHECK (late_sequenced IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (ownership_scope_key, request_id),
  FOREIGN KEY (owner_user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (owner_service_account_id) REFERENCES service_accounts(service_account_id) ON DELETE CASCADE
);

CREATE INDEX idx_attribution_request_sessions_hash
  ON attribution_request_sessions(session_hash, ownership_scope_key, request_id);

CREATE INDEX idx_agent_session_requests_execution_time
  ON agent_session_requests(execution_id, occurred_at)
  WHERE execution_id IS NOT NULL;
