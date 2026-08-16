import { createFileRoute, redirect } from '@tanstack/react-router'

// Legacy route — the servers view now lives in the unified /mcp workspace.
// Redirect (preserving any deep-linked server_id) so old bookmarks keep working.
export const Route = createFileRoute('/mcp/servers')({
  beforeLoad: ({ location }) => {
    const rawSearch = location.search as Record<string, unknown>
    const serverId = typeof rawSearch.server_id === 'string' ? rawSearch.server_id : undefined
    throw redirect({ to: '/mcp', search: { tab: 'servers', server_id: serverId } })
  },
})
