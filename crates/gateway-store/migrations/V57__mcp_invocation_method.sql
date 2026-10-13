-- Record which MCP JSON-RPC method each invocation row covers. Rows written
-- before this column existed were either tools/call, tools/list (logged under
-- the tool key `tools/list`), or unparseable requests.
ALTER TABLE mcp_tool_invocations
  ADD COLUMN mcp_method TEXT NOT NULL DEFAULT 'tools/call';

UPDATE mcp_tool_invocations
  SET mcp_method = 'tools/list'
  WHERE tool_id IS NULL AND tool_display_key = 'tools/list';

UPDATE mcp_tool_invocations
  SET mcp_method = 'invalid'
  WHERE tool_id IS NULL AND error_code = 'invalid_json_rpc';

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_method_time_idx
  ON mcp_tool_invocations (mcp_method, occurred_at);
