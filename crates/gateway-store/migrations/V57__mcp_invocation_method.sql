-- Record which MCP JSON-RPC method each invocation row covers. Rows written
-- before this column existed were either tools/call, tools/list (logged under
-- the tool key `tools/list`), or unparseable requests.
ALTER TABLE mcp_tool_invocations
  ADD COLUMN mcp_method TEXT NOT NULL DEFAULT 'tools/call';

-- One pass over the table; an unparseable request wins over the tools/list key.
UPDATE mcp_tool_invocations
  SET mcp_method = CASE
    WHEN error_code = 'invalid_json_rpc' THEN 'invalid'
    ELSE 'tools/list'
  END
  WHERE tool_id IS NULL
    AND (error_code = 'invalid_json_rpc' OR tool_display_key = 'tools/list');

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_method_time_idx
  ON mcp_tool_invocations (mcp_method, occurred_at);
