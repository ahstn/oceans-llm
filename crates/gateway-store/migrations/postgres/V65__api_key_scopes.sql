-- Capability scopes and an MCP server list per API key (#446). Both columns are
-- space-delimited lists. NULL keeps the behaviour of keys created before scopes
-- existed: every scope, and every MCP server the owner's grants reach.
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS scopes TEXT;
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS mcp_server_keys TEXT;
