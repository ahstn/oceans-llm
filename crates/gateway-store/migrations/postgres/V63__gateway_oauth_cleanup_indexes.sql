-- Support the hourly MCP client OAuth cleanup: expired-token deletion, the
-- per-client check for authorization requests still in flight, and the
-- retention filter on dynamically registered clients.

CREATE INDEX IF NOT EXISTS gateway_oauth_tokens_expiry_idx
  ON gateway_oauth_tokens (expires_at);

CREATE INDEX IF NOT EXISTS gateway_oauth_authorization_requests_client_idx
  ON gateway_oauth_authorization_requests (client_id);

CREATE INDEX IF NOT EXISTS gateway_oauth_clients_registration_idx
  ON gateway_oauth_clients (registration_kind, created_at);
