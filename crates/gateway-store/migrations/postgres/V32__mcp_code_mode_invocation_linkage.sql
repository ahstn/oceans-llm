ALTER TABLE mcp_tool_invocations
  ADD COLUMN parent_invocation_id TEXT
    REFERENCES mcp_tool_invocations(mcp_tool_invocation_id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS mcp_tool_invocations_parent_invocation_idx
  ON mcp_tool_invocations (parent_invocation_id, occurred_at);

-- Durable MCP sessions are bound to the gateway surface they were
-- initialized against; `/mcp` (aggregate) and `/code-mode-mcp` (code_mode)
-- sessions are never interchangeable.
ALTER TABLE mcp_aggregate_sessions
  ADD COLUMN surface TEXT NOT NULL DEFAULT 'aggregate'
    CHECK (surface IN ('aggregate', 'code_mode'));
