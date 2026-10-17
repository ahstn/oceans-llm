-- Delegated MCP server ownership (docs/adr/2026-10-17-delegated-mcp-server-ownership.md).
-- `owner_team_id` is NULL for a platform-owned server. Existing servers become
-- platform-owned and published to the organisation, so their access is unchanged.
ALTER TABLE external_mcp_servers ADD COLUMN IF NOT EXISTS owner_team_id TEXT
  REFERENCES teams(team_id) ON DELETE SET NULL;
ALTER TABLE external_mcp_servers ADD COLUMN IF NOT EXISTS owner_contact TEXT;
ALTER TABLE external_mcp_servers ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'organisation'
  CHECK (visibility IN ('team', 'organisation'));
ALTER TABLE external_mcp_servers ADD CONSTRAINT external_mcp_servers_platform_visibility_check
  CHECK (owner_team_id IS NOT NULL OR visibility = 'organisation');

CREATE INDEX IF NOT EXISTS external_mcp_servers_owner_team_idx
  ON external_mcp_servers (owner_team_id, server_key);
