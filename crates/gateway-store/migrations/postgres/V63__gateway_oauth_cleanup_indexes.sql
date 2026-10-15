-- Support the hourly MCP client OAuth cleanup: expired-token deletion and the
-- per-client check for authorization requests still in flight.

CREATE INDEX IF NOT EXISTS gateway_oauth_tokens_expiry_idx
  ON gateway_oauth_tokens (expires_at);

CREATE INDEX IF NOT EXISTS gateway_oauth_authorization_requests_client_idx
  ON gateway_oauth_authorization_requests (client_id);
