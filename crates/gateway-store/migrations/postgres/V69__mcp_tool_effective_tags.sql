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
    CASE
      WHEN jsonb_typeof(annotation_overrides_json -> 'readOnlyHint') = 'boolean'
        THEN (annotation_overrides_json ->> 'readOnlyHint')::boolean
      WHEN jsonb_typeof(annotations_json -> 'readOnlyHint') = 'boolean'
        THEN (annotations_json ->> 'readOnlyHint')::boolean
    END AS read_only,
    CASE
      WHEN jsonb_typeof(annotation_overrides_json -> 'destructiveHint') = 'boolean'
        THEN (annotation_overrides_json ->> 'destructiveHint')::boolean
      WHEN jsonb_typeof(annotations_json -> 'destructiveHint') = 'boolean'
        THEN (annotations_json ->> 'destructiveHint')::boolean
    END AS destructive,
    CASE
      WHEN jsonb_typeof(annotation_overrides_json -> 'idempotentHint') = 'boolean'
        THEN (annotation_overrides_json ->> 'idempotentHint')::boolean
      WHEN jsonb_typeof(annotations_json -> 'idempotentHint') = 'boolean'
        THEN (annotations_json ->> 'idempotentHint')::boolean
    END AS idempotent,
    CASE
      WHEN jsonb_typeof(annotation_overrides_json -> 'openWorldHint') = 'boolean'
        THEN (annotation_overrides_json ->> 'openWorldHint')::boolean
      WHEN jsonb_typeof(annotations_json -> 'openWorldHint') = 'boolean'
        THEN (annotations_json ->> 'openWorldHint')::boolean
    END AS open_world
  FROM external_mcp_tools
)
INSERT INTO mcp_tool_effective_tags (mcp_tool_id, tag)
SELECT mcp_tool_id, 'read_only' FROM hints WHERE COALESCE(read_only, false)
UNION ALL
SELECT mcp_tool_id, 'destructive' FROM hints
  WHERE NOT COALESCE(read_only, false) AND COALESCE(destructive, true)
UNION ALL
SELECT mcp_tool_id, 'idempotent' FROM hints WHERE COALESCE(idempotent, false)
UNION ALL
SELECT mcp_tool_id, 'open_world' FROM hints WHERE COALESCE(open_world, true)
UNION ALL
SELECT t.mcp_tool_id, tags.tag
  FROM external_mcp_tools t, jsonb_array_elements_text(t.admin_tags_json) AS tags(tag)
ON CONFLICT DO NOTHING;
