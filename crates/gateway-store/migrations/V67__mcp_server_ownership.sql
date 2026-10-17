-- Delegated MCP server ownership (docs/adr/2026-10-17-delegated-mcp-server-ownership.md).
-- `owner_team_id` is NULL for a platform-owned server. Existing servers become
-- platform-owned and published to the organisation, so their access is unchanged.
-- The team reference restricts deletion: a team that owns servers cannot be
-- deleted until a platform admin moves them, so a team-only server never loses
-- its owner.
-- A platform-owned server is always `organisation`. SQLite cannot add a
-- cross-column CHECK to an existing table, so triggers enforce it, matching the
-- Postgres constraint.
ALTER TABLE external_mcp_servers ADD COLUMN owner_team_id TEXT
  REFERENCES teams(team_id);
ALTER TABLE external_mcp_servers ADD COLUMN owner_contact TEXT;
ALTER TABLE external_mcp_servers ADD COLUMN visibility TEXT NOT NULL DEFAULT 'organisation'
  CHECK (visibility IN ('team', 'organisation'));

CREATE INDEX IF NOT EXISTS external_mcp_servers_owner_team_idx
  ON external_mcp_servers (owner_team_id, server_key);

CREATE TRIGGER IF NOT EXISTS external_mcp_servers_platform_visibility_insert
BEFORE INSERT ON external_mcp_servers
WHEN NEW.owner_team_id IS NULL AND NEW.visibility <> 'organisation'
BEGIN
  SELECT RAISE(ABORT, 'a platform-owned MCP server must be visible to the organisation');
END;

CREATE TRIGGER IF NOT EXISTS external_mcp_servers_platform_visibility_update
BEFORE UPDATE OF owner_team_id, visibility ON external_mcp_servers
WHEN NEW.owner_team_id IS NULL AND NEW.visibility <> 'organisation'
BEGIN
  SELECT RAISE(ABORT, 'a platform-owned MCP server must be visible to the organisation');
END;
