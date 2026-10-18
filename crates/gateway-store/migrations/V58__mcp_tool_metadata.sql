-- Keep the optional members of upstream MCP tool definitions. Existing rows
-- start with metadata_captured = 0, so the first discovery that records their
-- metadata can update the schema hash without counting as a schema change.
ALTER TABLE external_mcp_tools ADD COLUMN title TEXT;
ALTER TABLE external_mcp_tools ADD COLUMN annotations_json TEXT;
ALTER TABLE external_mcp_tools ADD COLUMN output_schema_json TEXT;
ALTER TABLE external_mcp_tools
  ADD COLUMN metadata_captured INTEGER NOT NULL DEFAULT 0 CHECK (metadata_captured IN (0, 1));
