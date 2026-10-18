-- Admin-owned tool policy (#443). Discovery never writes these columns, so the
-- discovered annotations stay next to the admin's overrides.
ALTER TABLE external_mcp_tools ADD COLUMN admin_tags_json JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE external_mcp_tools ADD COLUMN annotation_overrides_json JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Grants and denials that select tools by tag. `server_scope` is a server ID,
-- or `*` for every server (exclude rules only).
CREATE TABLE IF NOT EXISTS mcp_tool_tag_rules (
  rule_id TEXT PRIMARY KEY,
  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('api_key', 'user', 'team', 'service_account')),
  subject_id TEXT NOT NULL,
  server_scope TEXT NOT NULL,
  tag TEXT NOT NULL,
  effect TEXT NOT NULL CHECK (effect IN ('include', 'exclude')),
  is_active BIGINT NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  revoked_at BIGINT,
  CHECK (effect = 'exclude' OR server_scope != '*')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_tool_tag_rules_active_unique
  ON mcp_tool_tag_rules(subject_kind, subject_id, server_scope, tag, effect)
  WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS idx_mcp_tool_tag_rules_subject
  ON mcp_tool_tag_rules(subject_kind, subject_id, is_active);
