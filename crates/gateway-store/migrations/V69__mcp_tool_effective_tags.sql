-- Each tool's effective tags (#443), so include tag rules select matching
-- tools by index instead of scanning a server's tools. The store rewrites a
-- tool's rows whenever its definition, annotation overrides or admin tags
-- change, from `gateway_core::mcp_tool_policy::effective_tags`.
CREATE TABLE IF NOT EXISTS mcp_tool_effective_tags (
  mcp_tool_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  PRIMARY KEY (mcp_tool_id, tag),
  FOREIGN KEY (mcp_tool_id) REFERENCES external_mcp_tools(mcp_tool_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_mcp_tool_effective_tags_tag
  ON mcp_tool_effective_tags(tag, mcp_tool_id);

-- Backfill with the same rules: an override wins over the discovered hint,
-- only booleans count, and undeclared hints fail safe (destructive and
-- open-world unless read-only or declared otherwise).
WITH hints AS (
  SELECT
    mcp_tool_id,
    CASE json_type(annotation_overrides_json, '$.readOnlyHint')
      WHEN 'true' THEN 1 WHEN 'false' THEN 0
      ELSE CASE json_type(annotations_json, '$.readOnlyHint') WHEN 'true' THEN 1 WHEN 'false' THEN 0 END
    END AS read_only,
    CASE json_type(annotation_overrides_json, '$.destructiveHint')
      WHEN 'true' THEN 1 WHEN 'false' THEN 0
      ELSE CASE json_type(annotations_json, '$.destructiveHint') WHEN 'true' THEN 1 WHEN 'false' THEN 0 END
    END AS destructive,
    CASE json_type(annotation_overrides_json, '$.idempotentHint')
      WHEN 'true' THEN 1 WHEN 'false' THEN 0
      ELSE CASE json_type(annotations_json, '$.idempotentHint') WHEN 'true' THEN 1 WHEN 'false' THEN 0 END
    END AS idempotent,
    CASE json_type(annotation_overrides_json, '$.openWorldHint')
      WHEN 'true' THEN 1 WHEN 'false' THEN 0
      ELSE CASE json_type(annotations_json, '$.openWorldHint') WHEN 'true' THEN 1 WHEN 'false' THEN 0 END
    END AS open_world
  FROM external_mcp_tools
)
INSERT OR IGNORE INTO mcp_tool_effective_tags (mcp_tool_id, tag)
SELECT mcp_tool_id, 'read_only' FROM hints WHERE COALESCE(read_only, 0) = 1
UNION ALL
SELECT mcp_tool_id, 'destructive' FROM hints
  WHERE COALESCE(read_only, 0) = 0 AND COALESCE(destructive, 1) = 1
UNION ALL
SELECT mcp_tool_id, 'idempotent' FROM hints WHERE COALESCE(idempotent, 0) = 1
UNION ALL
SELECT mcp_tool_id, 'open_world' FROM hints WHERE COALESCE(open_world, 1) = 1
UNION ALL
SELECT t.mcp_tool_id, tags.value FROM external_mcp_tools t, json_each(t.admin_tags_json) tags;
