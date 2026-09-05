import { createFileRoute, redirect, useRouter } from '@tanstack/react-router'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  getApiKeys,
  getMcpGrants,
  getMcpServers,
  getMcpToolsets,
  getRecommendedMcpServers,
  getUsers,
} from '@/server/admin-data.functions'
import { AccessTab } from './-access-tab'
import { ServersTab } from './-servers-tab'
import { McpNavigation } from './-navigation'
import { normalizeToolsetsSearch } from './-search'

type McpTab = 'servers' | 'toolsets' | 'access'

type McpSearch = {
  tab: McpTab
  server_id?: string
  toolset_id?: string
}

export const Route = createFileRoute('/mcp/')({
  validateSearch: (search: Record<string, unknown>) => normalizeMcpSearch(search),
  beforeLoad: ({ search }) => {
    if (search.tab === 'toolsets') {
      throw redirect({ to: '/mcp/toolsets', search: normalizeToolsetsSearch(search) })
    }
  },
  loader: async () => {
    const [servers, recommended, toolsets, grants, apiKeys, identity] = await Promise.all([
      getMcpServers({ data: { include_disabled: true } }),
      getRecommendedMcpServers(),
      getMcpToolsets({ data: { include_disabled: true } }),
      getMcpGrants(),
      getApiKeys(),
      getUsers(),
    ])
    return {
      servers: servers.data.items,
      recommended: recommended.data.items,
      toolsets: toolsets.data.items,
      grants: grants.data.items,
      apiKeys: apiKeys.data.items,
      users: apiKeys.data.users,
      serviceAccounts: apiKeys.data.service_accounts,
      teams: identity.data.teams,
    }
  },
  component: McpWorkspacePage,
})

export function McpWorkspacePage() {
  const data = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()

  const selectedServerId = search.server_id ?? null

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
            />
          ) : null}
        </CardContent>
      </Card>
    )

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <McpNavigation current={search.tab === 'access' ? 'access' : 'servers'} />
      {search.tab === 'access' ? (
        <header className="flex flex-col gap-2">
          <h1 className="text-2xl font-semibold tracking-tight">Access</h1>
          <p className="text-muted-foreground text-sm">
            Manage who can use your MCP tools and tool sets.
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
    tab: tab === 'toolsets' || tab === 'access' ? tab : 'servers',
    server_id:
      typeof search.server_id === 'string' && search.server_id ? search.server_id : undefined,
    toolset_id:
      typeof search.toolset_id === 'string' && search.toolset_id ? search.toolset_id : undefined,
  }
}
