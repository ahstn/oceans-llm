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
