import type { AdminTeamOption } from '@/types/api'

/**
 * What the signed-in admin may do in the MCP area. Platform admins manage every
 * server; team owners and admins manage their own team's servers and see the
 * organisation catalog read-only. The gateway enforces the same rules.
 */
export type McpWorkspaceScope = {
  platformAdmin: boolean
  /** Whether the caller may publish servers they manage to the organisation. */
  canPublish: boolean
  /** The team a team admin manages; `null` for a platform admin. */
  teamId: string | null
  /** Teams a platform admin can move a server to. */
  teams: AdminTeamOption[]
}

export const platformMcpScope: McpWorkspaceScope = {
  platformAdmin: true,
  canPublish: true,
  teamId: null,
  teams: [],
}
