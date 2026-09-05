import { createFileRoute, useRouter } from '@tanstack/react-router'
import { getMcpServers, getMcpToolsets } from '@/server/admin-data.functions'
import { ToolsetsTab } from './-toolsets-tab'
import { McpNavigation } from './-navigation'
import { normalizeToolsetsSearch } from './-search'

export const Route = createFileRoute('/mcp/toolsets')({
  validateSearch: normalizeToolsetsSearch,
  loader: async () => {
    const [servers, toolsets] = await Promise.all([
      getMcpServers({ data: { include_disabled: true } }),
      getMcpToolsets({ data: { include_disabled: true } }),
    ])
    return { servers: servers.data.items, toolsets: toolsets.data.items }
  },
  component: ToolsetsPage,
})

function ToolsetsPage() {
  const data = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  return (
    <div className="flex min-w-0 flex-col gap-6">
      <McpNavigation current="toolsets" />
      <ToolsetsTab
        toolsets={data.toolsets}
        servers={data.servers}
        selectedToolsetId={search.toolset_id ?? null}
        onSelectToolset={(toolsetId) => {
          void router.navigate({
            to: '/mcp/toolsets',
            search: (previous) => ({
              ...previous,
              toolset_id: toolsetId ?? undefined,
              tool_ids: undefined,
            }),
          })
        }}
        seedToolIds={search.tool_ids ?? []}
        onSeedConsumed={() => {
          void router.navigate({
            to: '/mcp/toolsets',
            search: (previous) => ({ ...previous, tool_ids: undefined }),
            replace: true,
          })
        }}
      />
    </div>
  )
}
