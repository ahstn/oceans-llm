import { useCallback, useEffect, useRef, useState } from 'react'

import type { ToolsetMembershipState } from '@/components/mcp/toolset-workbench'
import { getMcpToolsetTools, saveMcpToolsetTools } from '@/server/admin-data.functions'
import type { McpToolsetView } from '@/types/api'

type MembershipEntry = ToolsetMembershipState & { savedToolIds: string[] }

function sameMembers(left: string[], right: string[]) {
  const rightIds = new Set(right)
  return left.length === rightIds.size && left.every((id) => rightIds.has(id))
}

function loadedMembership(toolIds: string[]): MembershipEntry {
  const ids = [...new Set(toolIds)]
  return {
    toolIds: ids,
    savedToolIds: ids,
    dirty: false,
    loading: false,
    saving: false,
    error: null,
  }
}

/** Keeps each draft with its saved snapshot so switching sets never discards edits. */
export function useToolsetMemberships(toolsets: McpToolsetView[]) {
  const [entries, setEntries] = useState<Record<string, MembershipEntry>>({})
  const attempts = useRef(new Map<string, number>())
  const mounted = useRef(false)
  const toolsetIds = toolsets.map((toolset) => toolset.id).join(',')

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  const reload = useCallback((toolsetId: string) => {
    const attempt = (attempts.current.get(toolsetId) ?? 0) + 1
    attempts.current.set(toolsetId, attempt)
    setEntries((current) => ({
      ...current,
      [toolsetId]: { ...(current[toolsetId] ?? loadedMembership([])), loading: true, error: null },
    }))
    void getMcpToolsetTools({ data: { toolsetId } })
      .then(({ data }) => {
        if (!mounted.current || attempts.current.get(toolsetId) !== attempt) return
        setEntries((current) => {
          const saved = loadedMembership(data.tool_ids)
          const draft = current[toolsetId]
          return {
            ...current,
            [toolsetId]: draft?.dirty
              ? {
                  ...saved,
                  toolIds: draft.toolIds,
                  dirty: !sameMembers(draft.toolIds, saved.toolIds),
                }
              : saved,
          }
        })
      })
      .catch((cause: unknown) => {
        if (!mounted.current || attempts.current.get(toolsetId) !== attempt) return
        setEntries((current) => ({
          ...current,
          [toolsetId]: {
            ...current[toolsetId]!,
            loading: false,
            error: cause instanceof Error ? cause.message : 'Failed to load saved tools',
          },
        }))
      })
  }, [])

  useEffect(() => {
    for (const id of toolsetIds.split(',').filter(Boolean)) {
      if (!attempts.current.has(id)) reload(id)
    }
  }, [toolsetIds, reload])

  function initialize(toolsetId: string) {
    attempts.current.set(toolsetId, (attempts.current.get(toolsetId) ?? 0) + 1)
    setEntries((current) => ({ ...current, [toolsetId]: loadedMembership([]) }))
  }

  function update(toolsetId: string, change: (ids: string[]) => string[]) {
    setEntries((current) => {
      const entry = current[toolsetId]
      if (!entry || entry.loading || entry.error || entry.saving) return current
      const toolIds = [...new Set(change(entry.toolIds))]
      return {
        ...current,
        [toolsetId]: { ...entry, toolIds, dirty: !sameMembers(toolIds, entry.savedToolIds) },
      }
    })
  }

  async function save(toolsetId: string) {
    const entry = entries[toolsetId]
    if (!entry || entry.loading || entry.error || entry.saving || !entry.dirty) return null
    const submittedIds = [...entry.toolIds]
    setEntries((current) => ({
      ...current,
      [toolsetId]: { ...current[toolsetId]!, saving: true },
    }))
    try {
      const { data } = await saveMcpToolsetTools({ data: { toolsetId, toolIds: submittedIds } })
      if (mounted.current) {
        setEntries((current) => {
          const saved = loadedMembership(data.tool_ids)
          const latest = current[toolsetId]!
          const toolIds = sameMembers(latest.toolIds, submittedIds) ? saved.toolIds : latest.toolIds
          return {
            ...current,
            [toolsetId]: { ...saved, toolIds, dirty: !sameMembers(toolIds, saved.toolIds) },
          }
        })
      }
      return data.tool_ids.length
    } finally {
      if (mounted.current) {
        setEntries((current) => ({
          ...current,
          [toolsetId]: { ...current[toolsetId]!, saving: false },
        }))
      }
    }
  }

  return { entries, reload, initialize, update, save }
}
