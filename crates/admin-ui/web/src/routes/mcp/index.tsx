import { createFileRoute, redirect, useRouter } from '@tanstack/react-router'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import {
  getApiKeys,
  getMcpGrants,
  getMcpServers,
  getMcpToolsets,
  getRecommendedMcpServers,
  getTeamDirectory,
  getUsers,
} from '@/server/admin-data.functions'
import type { AdminTeamOption, ApiKeyOwnerUserView, AuthSessionView } from '@/types/api'
import { AccessTab } from './-access-tab'
import { ServersTab } from './-servers-tab'
import { McpNavigation } from './-navigation'
import type { McpWorkspaceScope } from './-scope'
import { normalizeToolsetsSearch, type ToolsetsSearch } from './-search'

type McpTab = 'servers' | 'toolsets' | 'access'

type McpSearch = ToolsetsSearch & {
  tab: McpTab
  server_id?: string
}

export const Route = createFileRoute('/mcp/')({
  validateSearch: (search: Record<string, unknown>) => normalizeMcpSearch(search),
  beforeLoad: ({ search }) => {
    if (search.tab === 'toolsets') {
      throw redirect({ to: '/mcp/toolsets', search: normalizeToolsetsSearch(search) })
    }
  },
  loader: async ({ context }) => {
    const platformAdmin = isPlatformAdminSession(context.session)
    const [servers, recommended, toolsets, grants, apiKeys, directory] = await Promise.all([
      getMcpServers({ data: { include_disabled: true } }),
      getRecommendedMcpServers(),
      // Tool sets and the full identity list are platform-admin only.
      platformAdmin ? getMcpToolsets({ data: { include_disabled: true } }) : null,
      getMcpGrants(),
      getApiKeys(),
      platformAdmin ? getUsers().then(toPlatformDirectory) : loadTeamDirectory(context.session),
    ])
    return {
      platformAdmin,
      canPublish: servers.data.can_publish_to_organisation,
      teamId: platformAdmin ? null : (context.session?.team_id ?? null),
      servers: servers.data.items,
      recommended: recommended.data.items,
      toolsets: toolsets?.data.items ?? [],
      grants: grants.data.items,
      apiKeys: apiKeys.data.items,
      users: directory.users ?? apiKeys.data.users,
      serviceAccounts: apiKeys.data.service_accounts,
      teams: directory.teams,
    }
  },
  component: McpWorkspacePage,
})

export function McpWorkspacePage() {
  const data = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()

  const selectedServerId = search.server_id ?? null
  const scope: McpWorkspaceScope = {
    platformAdmin: data.platformAdmin,
    canPublish: data.canPublish,
    teamId: data.teamId,
    teams: data.platformAdmin ? data.teams : [],
  }

  function applySearch(next: Partial<McpSearch>) {
    void router.navigate({ to: '/mcp', search: normalizeMcpSearch({ ...search, ...next }) })
  }

  function handleAddToToolset(toolIds: string[]) {
    void router.navigate({ to: '/mcp/toolsets', search: { tool_ids: toolIds } })
  }

  const workspaceContent =
    search.tab === 'servers' ? (
      <ServersTab
        servers={data.servers}
        recommended={data.recommended}
        selectedServerId={selectedServerId}
        scope={scope}
        onSelectServer={(serverId) => applySearch({ server_id: serverId ?? undefined })}
        onAddToToolset={handleAddToToolset}
      />
    ) : (
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Access rules</CardTitle>
          <CardDescription>
            Choose which people and accounts can use each tool or group of tools.
          </CardDescription>
        </CardHeader>
        <CardContent className="min-w-0">
          {search.tab === 'access' ? (
            <AccessTab
              grants={data.grants}
              servers={data.servers}
              toolsets={data.toolsets}
              subjects={{
                apiKeys: data.apiKeys,
                users: data.users,
                serviceAccounts: data.serviceAccounts,
                teams: data.teams,
              }}
              scope={scope}
            />
          ) : null}
        </CardContent>
      </Card>
    )

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <McpNavigation
        current={search.tab === 'access' ? 'access' : 'servers'}
        showToolsets={data.platformAdmin}
      />
      {search.tab === 'access' ? (
        <header className="flex flex-col gap-2">
          <h1 className="text-2xl font-semibold tracking-tight">Access</h1>
          <p className="text-muted-foreground text-sm">
            {data.platformAdmin
              ? 'Manage who can use your MCP tools and tool sets.'
              : "Manage who in your team can use your team's MCP tools."}
          </p>
        </header>
      ) : null}
      {workspaceContent}
    </div>
  )
}

function normalizeMcpSearch(search: Record<string, unknown>): McpSearch {
  const tab = search.tab
  return {
    ...normalizeToolsetsSearch(search),
    tab: tab === 'toolsets' || tab === 'access' ? tab : 'servers',
    server_id:
      typeof search.server_id === 'string' && search.server_id ? search.server_id : undefined,
  }
}

type McpSubjectDirectory = {
  teams: AdminTeamOption[]
  /** `undefined` falls back to the owners the API keys payload lists. */
  users?: ApiKeyOwnerUserView[]
}

function toPlatformDirectory(identity: Awaited<ReturnType<typeof getUsers>>): McpSubjectDirectory {
  return { teams: identity.data.teams }
}

/** A team admin grants only to their own team and its members. */
async function loadTeamDirectory(
  session: AuthSessionView | null | undefined,
): Promise<McpSubjectDirectory> {
  const response = await getTeamDirectory()
  const team = response.data.teams.find((entry) => entry.id === session?.team_id)
  if (!team) return { teams: [], users: [] }
  return {
    teams: [{ id: team.id, name: team.name }],
    users: team.members.map((member) => ({
      id: member.id,
      name: member.name,
      email: member.email,
    })),
  }
}
