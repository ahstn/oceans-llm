import { useEffect, useMemo, useState, useTransition, type FormEvent } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  addMcpServer,
  disableExternalMcpServer,
  getMcpCredentialBindings,
  getMcpServers,
  getMcpServerTools,
  getRecommendedMcpServers,
  removeMcpCredentialBinding,
  refreshExternalMcpServerDiscovery,
  saveMcpServer,
  saveMcpCredentialBinding,
} from '@/server/admin-data.functions'
import type {
  McpCredentialBindingView,
  McpServerView,
  McpToolView,
  RecommendedMcpServerView,
} from '@/types/api'
import {
  MetricLabel,
  CredentialBindingsPanel,
  ServerDetail,
  ServerFormDialog,
  ServerStatusBadge,
  emptyCredentialBindingForm,
  emptyServerForm,
  formFromRecommended,
  formFromServer,
  formToCredentialBindingInput,
  formToCreateInput,
  formToUpdateInput,
  type CredentialBindingFormState,
  type ServerFormState,
} from './-components'

type McpServersSearch = {
  server_id?: string
}

export const Route = createFileRoute('/mcp/servers')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeMcpServersSearch(search),
  loader: async () => {
    const [servers, recommended] = await Promise.all([
      getMcpServers({ data: { include_disabled: true } }),
      getRecommendedMcpServers(),
    ])
    return {
      servers: servers.data.items,
      recommended: recommended.data.items,
    }
  },
  component: McpServersPage,
})

export function McpServersPage() {
  const { servers, recommended } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [isPending, startTransition] = useTransition()
  const [selectedServerId, setSelectedServerId] = useState<string | null>(
    search.server_id ?? servers[0]?.id ?? null,
  )
  const [tools, setTools] = useState<McpToolView[]>([])
  const [toolsPending, setToolsPending] = useState(false)
  const [toolsError, setToolsError] = useState<string | null>(null)
  const [credentialBindings, setCredentialBindings] = useState<McpCredentialBindingView[]>([])
  const [credentialBindingsPending, setCredentialBindingsPending] = useState(false)
  const [credentialBindingsError, setCredentialBindingsError] = useState<string | null>(null)
  const [credentialForm, setCredentialForm] = useState<CredentialBindingFormState>(() =>
    emptyCredentialBindingForm(),
  )
  const [refreshStatus, setRefreshStatus] = useState<string | null>(null)
  const [refreshErrorSummary, setRefreshErrorSummary] = useState<string | null>(null)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editDialogOpen, setEditDialogOpen] = useState(false)
  const [formState, setFormState] = useState<ServerFormState>(() => emptyServerForm())

  const selectedServer = useMemo(
    () => servers.find((server) => server.id === selectedServerId) ?? servers[0] ?? null,
    [selectedServerId, servers],
  )

  useEffect(() => {
    if (!selectedServer) {
      setTools([])
      setToolsError(null)
      setToolsPending(false)
      return
    }

    let cancelled = false
    setToolsPending(true)
    setToolsError(null)
    void getMcpServerTools({
      data: { serverId: selectedServer.id, include_inactive: true },
    })
      .then((response) => {
        if (!cancelled) {
          setTools(response.data.items)
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setToolsError(error instanceof Error ? error.message : 'Failed to load MCP tools')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setToolsPending(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [selectedServer])

  useEffect(() => {
    if (!selectedServer) {
      setCredentialBindings([])
      setCredentialBindingsError(null)
      setCredentialBindingsPending(false)
      return
    }

    let cancelled = false
    setCredentialBindingsPending(true)
    setCredentialBindingsError(null)
    void loadCredentialBindings(selectedServer.id)
      .then((items) => {
        if (!cancelled) {
          setCredentialBindings(items)
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setCredentialBindingsError(
            error instanceof Error ? error.message : 'Failed to load MCP credential bindings',
          )
        }
      })
      .finally(() => {
        if (!cancelled) {
          setCredentialBindingsPending(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [selectedServer])

  async function loadCredentialBindings(serverId: string) {
    const response = await getMcpCredentialBindings({
      data: { server_id: serverId, include_revoked: true },
    })
    return response.data.items
  }

  function selectServer(serverId: string) {
    setSelectedServerId(serverId)
    setRefreshStatus(null)
    setRefreshErrorSummary(null)
    void router.navigate({
      to: '/mcp/servers',
      search: normalizeMcpServersSearch({ server_id: serverId }),
    })
  }

  async function selectServerAfterCreate(serverId: string) {
    setSelectedServerId(serverId)
    setRefreshStatus(null)
    setRefreshErrorSummary(null)
    await router.invalidate()
    await router.navigate({
      to: '/mcp/servers',
      search: normalizeMcpServersSearch({ server_id: serverId }),
    })
  }

  function openCreateDialog(server?: RecommendedMcpServerView) {
    setFormState(server ? formFromRecommended(server) : emptyServerForm())
    setCreateDialogOpen(true)
  }

  function openEditDialog(server: McpServerView) {
    setFormState(formFromServer(server))
    setEditDialogOpen(true)
  }

  function handleCreateServer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const input = formToCreateInput(formState)
    if (!input) {
      return
    }
    startTransition(async () => {
      try {
        const response = await addMcpServer({ data: input })
        toast.success('MCP server added')
        setCreateDialogOpen(false)
        await selectServerAfterCreate(response.data.server.id)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to add MCP server')
      }
    })
  }

  function handleImportRecommended(server: RecommendedMcpServerView) {
    startTransition(async () => {
      try {
        const response = await addMcpServer({
          data: { recommended_catalog_key: server.catalog_key },
        })
        toast.success('MCP server imported')
        await selectServerAfterCreate(response.data.server.id)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to import MCP server')
      }
    })
  }

  function handleUpdateServer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedServer) {
      return
    }
    const input = formToUpdateInput(formState)
    if (!input) {
      return
    }
    startTransition(async () => {
      try {
        const response = await saveMcpServer({
          data: { serverId: selectedServer.id, input },
        })
        toast.success('MCP server updated')
        setEditDialogOpen(false)
        setSelectedServerId(response.data.server.id)
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to update MCP server')
      }
    })
  }

  function handleDisableServer(server: McpServerView) {
    startTransition(async () => {
      try {
        const response = await disableExternalMcpServer({ data: { serverId: server.id } })
        toast.success('MCP server disabled')
        setSelectedServerId(response.data.server.id)
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to disable MCP server')
      }
    })
  }

  function handleRefreshDiscovery(server: McpServerView) {
    setRefreshStatus('pending')
    setRefreshErrorSummary(null)
    startTransition(async () => {
      try {
        const response = await refreshExternalMcpServerDiscovery({
          data: { serverId: server.id },
        })
        setTools(response.data.tools)
        setRefreshStatus(response.data.status)
        setRefreshErrorSummary(response.data.error_summary ?? null)
        if (response.data.status === 'success') {
          toast.success('Discovery refreshed')
        } else {
          toast.error(response.data.error_summary ?? `Discovery ${response.data.status}`)
        }
        await router.invalidate()
      } catch (error) {
        setRefreshStatus('failed')
        setRefreshErrorSummary(error instanceof Error ? error.message : 'Discovery refresh failed')
        toast.error(error instanceof Error ? error.message : 'Discovery refresh failed')
      }
    })
  }

  function handleCreateCredentialBinding(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedServer) {
      return
    }
    const input = formToCredentialBindingInput(selectedServer.id, credentialForm)
    if (!input) {
      return
    }
    startTransition(async () => {
      try {
        await saveMcpCredentialBinding({ data: input })
        toast.success('Credential binding saved')
        setCredentialForm(emptyCredentialBindingForm())
        setCredentialBindings(await loadCredentialBindings(selectedServer.id))
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save credential binding')
      }
    })
  }

  function handleRevokeCredentialBinding(binding: McpCredentialBindingView) {
    startTransition(async () => {
      try {
        await removeMcpCredentialBinding({ data: { credentialBindingId: binding.id } })
        toast.success('Credential binding revoked')
        if (selectedServer) {
          setCredentialBindings(await loadCredentialBindings(selectedServer.id))
        }
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to revoke credential binding')
      }
    })
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Card className="min-w-0">
        <CardHeader className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
          <div className="flex min-w-0 flex-col gap-1">
            <CardTitle>MCP Servers</CardTitle>
            <CardDescription>
              Register Streamable HTTP servers, monitor discovery, and inspect exposed tools.
            </CardDescription>
          </div>
          <Button type="button" onClick={() => openCreateDialog()}>
            Add server
          </Button>
        </CardHeader>
        <CardContent className="grid min-w-0 gap-4 xl:grid-cols-[22rem_minmax(0,1fr)]">
          <ServerList
            servers={servers}
            selectedServer={selectedServer}
            onSelectServer={selectServer}
          />
          <div className="min-w-0">
            {selectedServer ? (
              <div className="flex min-w-0 flex-col gap-4">
                <ServerDetail
                  server={selectedServer}
                  tools={tools}
                  toolsPending={toolsPending}
                  toolsError={toolsError}
                  refreshStatus={refreshStatus}
                  refreshErrorSummary={refreshErrorSummary}
                  actionPending={isPending}
                  onEdit={openEditDialog}
                  onDisable={handleDisableServer}
                  onRefresh={handleRefreshDiscovery}
                />
                <CredentialBindingsPanel
                  bindings={credentialBindings}
                  form={credentialForm}
                  pending={isPending || credentialBindingsPending}
                  error={credentialBindingsError}
                  onFormChange={setCredentialForm}
                  onSubmit={handleCreateCredentialBinding}
                  onRevoke={handleRevokeCredentialBinding}
                />
              </div>
            ) : (
              <Empty>
                <EmptyHeader>
                  <EmptyTitle>Select an MCP server</EmptyTitle>
                  <EmptyDescription>Server diagnostics appear after registration.</EmptyDescription>
                </EmptyHeader>
              </Empty>
            )}
          </div>
          <RecommendedCatalog
            recommended={recommended}
            pending={isPending}
            onCustomize={openCreateDialog}
            onImport={handleImportRecommended}
          />
        </CardContent>
      </Card>

      <ServerFormDialog
        mode="create"
        open={createDialogOpen}
        pending={isPending}
        form={formState}
        onOpenChange={setCreateDialogOpen}
        onFormChange={setFormState}
        onSubmit={handleCreateServer}
      />

      <ServerFormDialog
        mode="edit"
        open={editDialogOpen}
        pending={isPending}
        form={formState}
        onOpenChange={setEditDialogOpen}
        onFormChange={setFormState}
        onSubmit={handleUpdateServer}
      />
    </div>
  )
}

function ServerList({
  servers,
  selectedServer,
  onSelectServer,
}: {
  servers: McpServerView[]
  selectedServer: McpServerView | null
  onSelectServer: (serverId: string) => void
}) {
  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex items-center justify-between gap-2 text-sm text-[var(--color-text-muted)]">
        <span>{servers.length} registered</span>
        <span>{servers.filter((server) => server.status === 'active').length} active</span>
      </div>
      {servers.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No MCP servers</EmptyTitle>
            <EmptyDescription>Add a catalog server or create a custom one.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="flex flex-col gap-2" data-testid="mcp-server-list">
          {servers.map((server) => (
            <button
              key={server.id}
              type="button"
              className={`rounded-md border px-3 py-3 text-left transition-colors ${
                selectedServer?.id === server.id
                  ? 'border-[var(--color-text)] bg-[var(--color-muted)]'
                  : 'border-[var(--color-border)] hover:bg-[var(--color-muted)]'
              }`}
              onClick={() => onSelectServer(server.id)}
            >
              <div className="flex min-w-0 items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="truncate font-medium">{server.display_name}</div>
                  <div className="truncate text-xs text-[var(--color-text-muted)]">
                    {server.server_key}
                  </div>
                </div>
                <ServerStatusBadge status={server.status} />
              </div>
              <div className="mt-3 grid grid-cols-2 gap-2 text-xs text-[var(--color-text-muted)]">
                <MetricLabel label="Discovery" value={server.last_discovery_status ?? 'none'} />
                <MetricLabel label="Tools" value={String(server.last_tool_count ?? 0)} />
              </div>
              {server.last_error_summary ? (
                <div className="mt-2 line-clamp-2 text-xs text-[var(--color-danger)]">
                  {server.last_error_summary}
                </div>
              ) : null}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function RecommendedCatalog({
  recommended,
  pending,
  onCustomize,
  onImport,
}: {
  recommended: RecommendedMcpServerView[]
  pending: boolean
  onCustomize: (server: RecommendedMcpServerView) => void
  onImport: (server: RecommendedMcpServerView) => void
}) {
  if (recommended.length === 0) {
    return null
  }

  return (
    <div className="flex flex-col gap-2 border-t pt-3 xl:col-span-2">
      <div className="text-sm font-medium">Recommended catalog</div>
      <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
        {recommended.map((server) => (
          <div
            key={server.catalog_key}
            className="flex min-w-0 items-center justify-between gap-2 rounded-md border p-3"
          >
            <div className="min-w-0">
              <div className="truncate text-sm font-medium">{server.display_name}</div>
              <div className="truncate text-xs text-[var(--color-text-muted)]">
                {server.catalog_key}
              </div>
            </div>
            <div className="flex shrink-0 gap-2">
              <Button type="button" size="sm" variant="outline" onClick={() => onCustomize(server)}>
                Customize
              </Button>
              <Button type="button" size="sm" onClick={() => onImport(server)} disabled={pending}>
                Import
              </Button>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

function normalizeMcpServersSearch(search: Record<string, unknown>): McpServersSearch {
  return {
    server_id:
      typeof search.server_id === 'string' && search.server_id ? search.server_id : undefined,
  }
}
