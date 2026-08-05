import { createFileRoute, redirect } from '@tanstack/react-router'

// Legacy route — access management now lives in the unified /mcp workspace.
export const Route = createFileRoute('/mcp/access')({
  beforeLoad: () => {
    throw redirect({ to: '/mcp', search: { tab: 'access' } })
  },
})
