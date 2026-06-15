import { createFileRoute, redirect } from '@tanstack/react-router'

import { requireAdminSession } from '@/routes/-admin-guard'

// Legacy route — access management now lives in the unified /mcp workspace.
export const Route = createFileRoute('/mcp/access')({
  beforeLoad: async ({ location }) => {
    await requireAdminSession(location)
    throw redirect({ to: '/mcp', search: { tab: 'access' } })
  },
})
