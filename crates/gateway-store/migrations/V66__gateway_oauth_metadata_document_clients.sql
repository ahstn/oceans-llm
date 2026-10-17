-- MCP clients may identify themselves with a Client ID Metadata Document: an
-- https client_id that the gateway fetches. Each one gets a row here, keyed by
-- that URL, so authorization requests and grants can reference it like a
-- dynamically registered client. The row mirrors the last document fetched.
-- Such clients hold no shared secret.
--
-- SQLite cannot alter a CHECK constraint in place, so the table is rebuilt.
-- The migration runner turns foreign keys off for it, so dropping the old
-- table does not cascade into the requests and grants that reference it.
-- See docs/adr/2026-10-15-mcp-client-oauth-authorization.md.

CREATE TABLE gateway_oauth_clients_v66 (
  client_id TEXT PRIMARY KEY,
  registration_kind TEXT NOT NULL
    CHECK (registration_kind IN ('dynamic', 'metadata_document')),
  client_name TEXT,
  client_uri TEXT,
  logo_uri TEXT,
  redirect_uris_json TEXT NOT NULL,
  grant_types_json TEXT NOT NULL,
  token_endpoint_auth_method TEXT NOT NULL CHECK (
    token_endpoint_auth_method IN ('none', 'client_secret_post', 'client_secret_basic')
  ),
  client_secret_hash TEXT,
  software_id TEXT,
  software_version TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK ((token_endpoint_auth_method = 'none') = (client_secret_hash IS NULL)),
  CHECK (registration_kind <> 'metadata_document' OR token_endpoint_auth_method = 'none')
);

INSERT INTO gateway_oauth_clients_v66 (
  client_id, registration_kind, client_name, client_uri, logo_uri, redirect_uris_json,
  grant_types_json, token_endpoint_auth_method, client_secret_hash, software_id,
  software_version, created_at, updated_at
)
SELECT
  client_id, registration_kind, client_name, client_uri, logo_uri, redirect_uris_json,
  grant_types_json, token_endpoint_auth_method, client_secret_hash, software_id,
  software_version, created_at, updated_at
FROM gateway_oauth_clients;

DROP TABLE gateway_oauth_clients;
ALTER TABLE gateway_oauth_clients_v66 RENAME TO gateway_oauth_clients;

CREATE INDEX IF NOT EXISTS gateway_oauth_clients_registration_idx
  ON gateway_oauth_clients (registration_kind, created_at);

-- Cleanup ages metadata document clients by their last fetch, not by when
-- they were first seen, so it needs this index to find them.
CREATE INDEX IF NOT EXISTS gateway_oauth_clients_metadata_updated_idx
  ON gateway_oauth_clients (registration_kind, updated_at);
