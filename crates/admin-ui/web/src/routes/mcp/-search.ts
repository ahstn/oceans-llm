export type ToolsetsSearch = { toolset_id?: string; tool_ids?: string[] }

export function normalizeToolsetsSearch(search: Record<string, unknown>): ToolsetsSearch {
  const ids = Array.isArray(search.tool_ids) ? search.tool_ids : []
  const toolIds = [
    ...new Set(ids.filter((id): id is string => typeof id === 'string' && id.trim().length > 0)),
  ]
  return {
    toolset_id:
      typeof search.toolset_id === 'string' && search.toolset_id ? search.toolset_id : undefined,
    tool_ids: toolIds.length ? toolIds : undefined,
  }
}
