-- Tool versions and approval of changed tools (#444).
ALTER TABLE external_mcp_servers
  ADD COLUMN approve_tool_changes INTEGER NOT NULL DEFAULT 0 CHECK (approve_tool_changes IN (0, 1));

-- The tool row holds the served definition. `approved_schema_version` is NULL
-- until the tool is first approved; `pending_schema_version` names the version
-- waiting for review.
ALTER TABLE external_mcp_tools
  ADD COLUMN approval_status TEXT NOT NULL DEFAULT 'approved'
  CHECK (approval_status IN ('approved', 'pending_change', 'pending_review', 'rejected'));
ALTER TABLE external_mcp_tools ADD COLUMN approved_schema_version INTEGER;
ALTER TABLE external_mcp_tools ADD COLUMN pending_schema_version INTEGER;
UPDATE external_mcp_tools SET approved_schema_version = schema_version;

CREATE TABLE IF NOT EXISTS mcp_tool_versions (
  mcp_tool_id TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  schema_hash TEXT NOT NULL,
  description TEXT,
  input_schema_json TEXT NOT NULL,
  title TEXT,
  annotations_json TEXT,
  output_schema_json TEXT,
  status TEXT NOT NULL CHECK (status IN ('approved', 'pending', 'rejected', 'superseded')),
  discovered_at INTEGER NOT NULL,
  reviewed_at INTEGER,
  reviewed_by_user_id TEXT,
  PRIMARY KEY (mcp_tool_id, schema_version),
  FOREIGN KEY (mcp_tool_id) REFERENCES external_mcp_tools(mcp_tool_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_mcp_tool_versions_status
  ON mcp_tool_versions(status, mcp_tool_id);

-- Every existing tool starts with its current definition as an approved version.
INSERT INTO mcp_tool_versions (
  mcp_tool_id, schema_version, schema_hash, description, input_schema_json, title,
  annotations_json, output_schema_json, status, discovered_at
)
SELECT mcp_tool_id, schema_version, schema_hash, description, input_schema_json, title,
  annotations_json, output_schema_json, 'approved', last_discovered_at
FROM external_mcp_tools;

ALTER TABLE mcp_tool_invocations ADD COLUMN tool_schema_version INTEGER;
