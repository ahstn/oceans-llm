-- RFC 9207: the issuer an upstream authorization response must come from.
ALTER TABLE mcp_oauth_states ADD COLUMN expected_issuer TEXT;
ALTER TABLE mcp_oauth_states ADD COLUMN issuer_required INTEGER NOT NULL DEFAULT 0;
