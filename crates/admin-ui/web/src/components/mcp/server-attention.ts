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
