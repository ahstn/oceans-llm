-- MCP clients may identify themselves with a Client ID Metadata Document: an
-- https client_id that the gateway fetches. Each one gets a row here, keyed by
-- that URL, so authorization requests and grants can reference it like a
-- dynamically registered client. The row mirrors the last document fetched.
-- Such clients hold no shared secret.
-- See docs/adr/2026-10-15-mcp-client-oauth-authorization.md.

ALTER TABLE gateway_oauth_clients
  DROP CONSTRAINT IF EXISTS gateway_oauth_clients_registration_kind_check;

ALTER TABLE gateway_oauth_clients
  ADD CONSTRAINT gateway_oauth_clients_registration_kind_check
  CHECK (registration_kind IN ('dynamic', 'metadata_document'));

ALTER TABLE gateway_oauth_clients
  ADD CONSTRAINT gateway_oauth_clients_metadata_document_public_check
  CHECK (registration_kind <> 'metadata_document' OR token_endpoint_auth_method = 'none');
