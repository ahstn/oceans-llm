import type { McpServerView } from '@/types/api'

/**
 * An active server whose last discovery failed or is waiting on credentials. The server registry
 * counts and filters these, and both the registry row and the server details mark them destructive.
 * Disabled servers never need attention, whatever their last discovery said.
 */
export function needsAttention(server: Pick<McpServerView, 'status' | 'last_discovery_status'>) {
  return (
    server.status === 'active' &&
    ['failed', 'auth_required'].includes(server.last_discovery_status ?? '')
  )
}

const discoveryLabels: Record<string, string> = {
  success: 'Discovered',
  failed: 'Discovery failed',
  auth_required: 'Authentication required',
  disabled: 'Discovery disabled',
}

/** A sentence-case label for a server's last discovery status, shared by the registry and details. */
export function discoveryLabel(status: string | null | undefined) {
  if (!status) return 'Not discovered'
  const fallback = status.replaceAll('_', ' ')
  return discoveryLabels[status] ?? fallback.charAt(0).toUpperCase() + fallback.slice(1)
}
