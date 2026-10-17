-- Delegated MCP server ownership (docs/adr/2026-10-17-delegated-mcp-server-ownership.md).
-- `owner_team_id` is NULL for a platform-owned server. Existing servers become
-- platform-owned and published to the organisation, so their access is unchanged.
-- The team reference restricts deletion: a team that owns servers cannot be
-- deleted until a platform admin moves them, so a team-only server never loses
-- its owner.
ALTER TABLE external_mcp_servers ADD COLUMN IF NOT EXISTS owner_team_id TEXT
  REFERENCES teams(team_id);
ALTER TABLE external_mcp_servers ADD COLUMN IF NOT EXISTS owner_contact TEXT;
ALTER TABLE external_mcp_servers ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'organisation'
  CHECK (visibility IN ('team', 'organisation'));
ALTER TABLE external_mcp_servers ADD CONSTRAINT external_mcp_servers_platform_visibility_check
  CHECK (owner_team_id IS NOT NULL OR visibility = 'organisation');

CREATE INDEX IF NOT EXISTS external_mcp_servers_owner_team_idx
  ON external_mcp_servers (owner_team_id, server_key);
-- A team admin's registry is their team's servers or the organisation's
-- (`owner_team_id = ? OR visibility = 'organisation'`); this index serves the
-- second branch, so neither branch scans the table.
CREATE INDEX IF NOT EXISTS external_mcp_servers_visibility_idx
  ON external_mcp_servers (visibility, server_key);
