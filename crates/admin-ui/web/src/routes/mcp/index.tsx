import { useState } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { requireAdminSession } from '@/routes/-admin-guard'
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
import { ToolsetsTab } from './-toolsets-tab'
import { SegmentedTabs } from './-shell'

type McpTab = 'servers' | 'toolsets' | 'access'

type McpSearch = {
  tab: McpTab
  server_id?: string
  toolset_id?: string
}

const workspaceTabs = [
  { value: 'servers', label: 'Servers' },
  { value: 'toolsets', label: 'Toolsets' },
  { value: 'access', label: 'Access' },
]

export const Route = createFileRoute('/mcp/')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeMcpSearch(search),
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
  const [seedToolIds, setSeedToolIds] = useState<string[]>([])

  const selectedServerId = search.server_id ?? null
  const selectedToolsetId = search.toolset_id ?? null

  function applySearch(next: Partial<McpSearch>) {
    void router.navigate({ to: '/mcp', search: normalizeMcpSearch({ ...search, ...next }) })
  }

  function handleAddToToolset(toolIds: string[]) {
    setSeedToolIds(toolIds)
    applySearch({ tab: 'toolsets' })
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Card className="min-w-0">
        <CardHeader className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
          <div className="flex min-w-0 flex-col gap-1">
            <CardTitle>MCP</CardTitle>
            <CardDescription>
              Register servers, curate toolsets, and manage access in one workspace.
            </CardDescription>
          </div>
          <SegmentedTabs
            ariaLabel="MCP workspace sections"
            value={search.tab}
            onValueChange={(value) => applySearch({ tab: value as McpTab })}
            items={workspaceTabs}
          />
        </CardHeader>
        <CardContent className="min-w-0">
          {search.tab === 'servers' ? (
            <ServersTab
              servers={data.servers}
              recommended={data.recommended}
              selectedServerId={selectedServerId}
              onSelectServer={(serverId) => applySearch({ server_id: serverId ?? undefined })}
              onAddToToolset={handleAddToToolset}
            />
          ) : null}
          {search.tab === 'toolsets' ? (
            <ToolsetsTab
              toolsets={data.toolsets}
              servers={data.servers}
              selectedToolsetId={selectedToolsetId}
              onSelectToolset={(toolsetId) => applySearch({ toolset_id: toolsetId ?? undefined })}
              seedToolIds={seedToolIds}
              onSeedConsumed={() => setSeedToolIds([])}
            />
          ) : null}
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
