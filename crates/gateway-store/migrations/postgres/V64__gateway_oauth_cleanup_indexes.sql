-- Support the hourly MCP client OAuth cleanup: expired-token deletion, the
-- per-client check for authorization requests still in flight, and the
-- retention filter on dynamically registered clients. The two grant indexes
-- let the authorized clients listing read each grant's latest token and
-- usable expiry with an index seek, however much token history it keeps.

CREATE INDEX IF NOT EXISTS gateway_oauth_tokens_expiry_idx
  ON gateway_oauth_tokens (expires_at);

CREATE INDEX IF NOT EXISTS gateway_oauth_authorization_requests_client_idx
  ON gateway_oauth_authorization_requests (client_id);

CREATE INDEX IF NOT EXISTS gateway_oauth_clients_registration_idx
  ON gateway_oauth_clients (registration_kind, created_at);

CREATE INDEX IF NOT EXISTS gateway_oauth_tokens_grant_issued_idx
  ON gateway_oauth_tokens (grant_id, created_at);

CREATE INDEX IF NOT EXISTS gateway_oauth_tokens_grant_usable_idx
  ON gateway_oauth_tokens (grant_id, expires_at)
  WHERE consumed_at IS NULL;

-- The grant-issued index leads with grant_id, so it also serves grant
-- lookups and cascading deletes.
DROP INDEX IF EXISTS gateway_oauth_tokens_grant_idx;

-- The authorized clients listing walks only grants that are not revoked.
-- Grants are marked revoked once their approving session ends, so the long
-- audit history of dead grants never sits in front of the live ones.
CREATE INDEX IF NOT EXISTS gateway_oauth_grants_live_user_idx
  ON gateway_oauth_grants (user_id, created_at)
  WHERE revoked_at IS NULL;

-- A session's expiry never changes, so each grant keeps a copy. Cleanup finds
-- grants whose session expired with an index range scan instead of joining
-- every live grant to its session. Revoking a session revokes its grants at
-- the same time.
ALTER TABLE gateway_oauth_grants ADD COLUMN IF NOT EXISTS session_expires_at BIGINT;

UPDATE gateway_oauth_grants
SET session_expires_at = (SELECT s.expires_at FROM user_sessions s
                          WHERE s.session_id = gateway_oauth_grants.user_session_id);

UPDATE gateway_oauth_grants
SET revoked_at = (SELECT s.revoked_at FROM user_sessions s
                  WHERE s.session_id = gateway_oauth_grants.user_session_id),
    updated_at = (SELECT s.revoked_at FROM user_sessions s
                  WHERE s.session_id = gateway_oauth_grants.user_session_id),
    revoked_reason = 'session_ended'
WHERE revoked_at IS NULL
  AND EXISTS (SELECT 1 FROM user_sessions s
              WHERE s.session_id = gateway_oauth_grants.user_session_id
                AND s.revoked_at IS NOT NULL);

CREATE INDEX IF NOT EXISTS gateway_oauth_grants_session_expiry_idx
  ON gateway_oauth_grants (session_expires_at)
  WHERE revoked_at IS NULL;
