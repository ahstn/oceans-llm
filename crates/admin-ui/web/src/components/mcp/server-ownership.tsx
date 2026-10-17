import { Badge } from '@/components/ui/badge'
import type { McpServerView } from '@/types/api'

export const PLATFORM_OWNER_LABEL = 'Platform admins'

/**
 * The owning team's name, or "Platform admins" for a platform-owned server. The
 * label avoids clashing with a team that is itself named "Platform".
 */
export function ownerLabel(server: Pick<McpServerView, 'owner_team_id' | 'owner_team_name'>) {
  if (!server.owner_team_id) return PLATFORM_OWNER_LABEL
  return server.owner_team_name ?? 'Unknown team'
}

export function visibilityLabel(visibility: string) {
  return visibility === 'team' ? 'Team only' : 'Organisation'
}

export function VisibilityBadge({ visibility }: { visibility: string }) {
  return (
    <Badge variant={visibility === 'team' ? 'outline' : 'secondary'}>
      {visibilityLabel(visibility)}
    </Badge>
  )
}

/** One entry per owner present in `servers`, platform first. */
export function ownerOptions(servers: McpServerView[]) {
  const teams = new Map<string, string>()
  let platform = false
  for (const server of servers) {
    if (server.owner_team_id) {
      teams.set(server.owner_team_id, ownerLabel(server))
    } else {
      platform = true
    }
  }
  return [
    ...(platform ? [{ value: 'platform', label: PLATFORM_OWNER_LABEL }] : []),
    ...[...teams.entries()]
      .map(([value, label]) => ({ value, label }))
      .sort((left, right) => left.label.localeCompare(right.label)),
  ]
}

export function matchesOwner(server: McpServerView, owner: string) {
  if (owner === 'all') return true
  if (owner === 'platform') return !server.owner_team_id
  return server.owner_team_id === owner
}
