ALTER TABLE request_logs
  ADD COLUMN referenced_mcp_server_count INTEGER;

ALTER TABLE request_logs
  ADD COLUMN exposed_tool_count INTEGER;

ALTER TABLE request_logs
  ADD COLUMN invoked_tool_count INTEGER;

ALTER TABLE request_logs
  ADD COLUMN filtered_tool_count INTEGER;
