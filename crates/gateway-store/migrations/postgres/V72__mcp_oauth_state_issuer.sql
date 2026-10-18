-- RFC 9207: the issuer an upstream authorization response must come from.
ALTER TABLE mcp_oauth_states ADD COLUMN IF NOT EXISTS expected_issuer TEXT;
ALTER TABLE mcp_oauth_states ADD COLUMN IF NOT EXISTS issuer_required BOOLEAN NOT NULL DEFAULT FALSE;
