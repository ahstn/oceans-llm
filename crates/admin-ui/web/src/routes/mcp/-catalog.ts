import { useEffect, useMemo, useState } from 'react'

import { getMcpServerTools } from '@/server/admin-data.functions'
import type { McpServerView, McpToolView } from '@/types/api'

export type ToolCatalog = {
  tools: McpToolView[]
  byServer: Map<string, McpToolView[]>
  byId: Map<string, McpToolView>
  pending: boolean
  error: string | null
  reload: () => void
}

/**
 * Loads the active-tool catalog across every active server in parallel. Shared
 * by the Toolsets membership editor and the Access grant/effective pickers so
 * tools can be chosen by name instead of by pasting UUIDs. Inactive tools are
 * excluded — toolsets and grants only ever reference callable tools.
 */
export function useToolCatalog(servers: McpServerView[]): ToolCatalog {
  const [tools, setTools] = useState<McpToolView[]>([])
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [nonce, setNonce] = useState(0)

  const activeServerIds = useMemo(
    () => servers.filter((server) => server.status === 'active').map((server) => server.id),
    [servers],
  )
  const serverKey = activeServerIds.join(',')

  useEffect(() => {
    if (activeServerIds.length === 0) {
      setTools([])
      setError(null)
      setPending(false)
      return
    }

    let cancelled = false
    setPending(true)
    setError(null)
    void Promise.all(
      activeServerIds.map((serverId) =>
        getMcpServerTools({ data: { serverId, include_inactive: false } }).then(
          (response) => response.data.items,
        ),
      ),
    )
      .then((results) => {
        if (!cancelled) {
          setTools(results.flat())
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setTools([])
          setError(cause instanceof Error ? cause.message : 'Failed to load MCP tools')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setPending(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [serverKey, nonce])

  const byServer = useMemo(() => {
    const map = new Map<string, McpToolView[]>()
    for (const tool of tools) {
      const existing = map.get(tool.server_id)
      if (existing) {
        existing.push(tool)
      } else {
        map.set(tool.server_id, [tool])
      }
    }
    return map
  }, [tools])

  const byId = useMemo(() => new Map(tools.map((tool) => [tool.id, tool])), [tools])

  return {
    tools,
    byServer,
    byId,
    pending,
    error,
    reload: () => setNonce((current) => current + 1),
  }
}
