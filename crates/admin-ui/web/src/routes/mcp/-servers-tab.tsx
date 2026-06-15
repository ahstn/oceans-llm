import { useEffect, useState, useTransition, type CSSProperties, type FormEvent } from 'react'
import {
  Cancel01Icon,
  Configuration01Icon,
  Delete02Icon,
  Edit02Icon,
  McpServerIcon,
  RefreshIcon,
  ShieldKeyIcon,
  ToolsIcon,
  ViewIcon,
} from '@hugeicons/core-free-icons'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
} from '@/components/ui/sidebar'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  addMcpServer,
  disableExternalMcpServer,
  getMcpCredentialBindings,
  getMcpServerTools,
  refreshExternalMcpServerDiscovery,
  removeMcpCredentialBinding,
  saveMcpCredentialBinding,
  saveMcpServer,
} from '@/server/admin-data.functions'
import type {
  McpCredentialBindingView,
  McpServerView,
  McpToolView,
  RecommendedMcpServerView,
} from '@/types/api'
import {
  CredentialBindingsPanel,
  DiscoveryStatusBadge,
  ServerFormDialog,
  ServerFormFields,
  ServerOverviewPanel,
  ServerStatusBadge,
  ServerToolsPanel,
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

type ServerSection = 'overview' | 'configuration' | 'tools' | 'credentials'

const serverSections = [
  { value: 'overview', label: 'Overview', icon: ViewIcon },
  { value: 'configuration', label: 'Configuration', icon: Configuration01Icon },
  { value: 'tools', label: 'Tools', icon: ToolsIcon },
  { value: 'credentials', label: 'Credentials', icon: ShieldKeyIcon },
] as const

export function ServersTab({
  servers,
  recommended,
  selectedServerId,
  onSelectServer,
  onAddToToolset,
}: {
  servers: McpServerView[]
  recommended: RecommendedMcpServerView[]
  selectedServerId: string | null
  onSelectServer: (serverId: string | null) => void
  onAddToToolset: (toolIds: string[]) => void
}) {
  const router = useRouter()
  const [isPending, startTransition] = useTransition()
  const [section, setSection] = useState<ServerSection>('overview')
  const [tools, setTools] = useState<McpToolView[]>([])
  const [toolsPending, setToolsPending] = useState(false)
  const [toolsError, setToolsError] = useState<string | null>(null)
  const [selectedToolIds, setSelectedToolIds] = useState<string[]>([])
  const [credentialBindings, setCredentialBindings] = useState<McpCredentialBindingView[]>([])
  const [credentialBindingsPending, setCredentialBindingsPending] = useState(false)
  const [credentialBindingsError, setCredentialBindingsError] = useState<string | null>(null)
  const [credentialForm, setCredentialForm] = useState<CredentialBindingFormState>(() =>
    emptyCredentialBindingForm(),
  )
  const [refreshStatus, setRefreshStatus] = useState<string | null>(null)
  const [refreshErrorSummary, setRefreshErrorSummary] = useState<string | null>(null)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editingServer, setEditingServer] = useState<McpServerView | null>(null)
  const [formState, setFormState] = useState<ServerFormState>(() => emptyServerForm())

  const selectedServer = servers.find((server) => server.id === selectedServerId) ?? null
  const activeCount = servers.filter((server) => server.status === 'active').length

  useEffect(() => {
    setSelectedToolIds([])
    if (!selectedServer) {
      setTools([])
      setToolsError(null)
      setToolsPending(false)
      return
    }

    let cancelled = false
    setToolsPending(true)
    setToolsError(null)
    void getMcpServerTools({ data: { serverId: selectedServer.id, include_inactive: true } })
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

  function handleSelectServer(serverId: string) {
    setRefreshStatus(null)
    setRefreshErrorSummary(null)
    setSection('overview')
    onSelectServer(serverId)
  }

  async function selectServerAfterMutation(serverId: string) {
    setRefreshStatus(null)
    setRefreshErrorSummary(null)
    await router.invalidate()
    onSelectServer(serverId)
  }

  function openCreateDialog(server?: RecommendedMcpServerView) {
    setFormState(server ? formFromRecommended(server) : emptyServerForm())
    setCreateDialogOpen(true)
  }

  function openEditDialog(server: McpServerView) {
    setEditingServer(server)
    setFormState(formFromServer(server))
    setSection('configuration')
    onSelectServer(server.id)
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
        await selectServerAfterMutation(response.data.server.id)
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
        await selectServerAfterMutation(response.data.server.id)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to import MCP server')
      }
    })
  }

  function handleUpdateServer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const server = editingServer ?? selectedServer
    if (!server) {
      return
    }
    const input = formToUpdateInput(formState)
    if (!input) {
      return
    }
    startTransition(async () => {
      try {
        const response = await saveMcpServer({ data: { serverId: server.id, input } })
        toast.success('MCP server updated')
        setEditingServer(null)
        await selectServerAfterMutation(response.data.server.id)
        setSection('overview')
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
        await selectServerAfterMutation(response.data.server.id)
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
        const response = await refreshExternalMcpServerDiscovery({ data: { serverId: server.id } })
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

  function toggleToolSelection(toolId: string) {
    setSelectedToolIds((current) =>
      current.includes(toolId) ? current.filter((id) => id !== toolId) : [...current, toolId],
    )
  }

  function handleAddToToolset() {
    if (selectedToolIds.length === 0) {
      return
    }
    onAddToToolset(selectedToolIds)
    setSelectedToolIds([])
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Card size="sm">
        <CardHeader className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
          <div className="flex min-w-0 flex-col gap-1">
            <CardTitle>Servers</CardTitle>
            <CardDescription>
              {servers.length} registered · {activeCount} active
            </CardDescription>
          </div>
          <Button type="button" onClick={() => openCreateDialog()}>
            Add server
          </Button>
        </CardHeader>
        <CardContent>
          <ServerTable
            servers={servers}
            actionPending={isPending}
            refreshStatus={refreshStatus}
            onSelectServer={handleSelectServer}
            onEdit={openEditDialog}
            onDisable={handleDisableServer}
            onRefresh={handleRefreshDiscovery}
          />
        </CardContent>
      </Card>

      <RecommendedCatalog
        recommended={recommended}
        pending={isPending}
        onCustomize={openCreateDialog}
        onImport={handleImportRecommended}
      />

      <ServerFormDialog
        mode="create"
        open={createDialogOpen}
        pending={isPending}
        form={formState}
        onOpenChange={setCreateDialogOpen}
        onFormChange={setFormState}
        onSubmit={handleCreateServer}
      />

      <ServerDetailDialog
        server={selectedServer}
        section={section}
        form={formState}
        tools={tools}
        toolsPending={toolsPending}
        toolsError={toolsError}
        selectedToolIds={selectedToolIds}
        credentialBindings={credentialBindings}
        credentialBindingsPending={credentialBindingsPending}
        credentialBindingsError={credentialBindingsError}
        credentialForm={credentialForm}
        refreshStatus={refreshStatus}
        refreshErrorSummary={refreshErrorSummary}
        actionPending={isPending}
        onOpenChange={(open) => {
          if (!open) {
            setEditingServer(null)
            onSelectServer(null)
          }
        }}
        onSectionChange={setSection}
        onFormChange={setFormState}
        onSubmit={handleUpdateServer}
        onEdit={openEditDialog}
        onDisable={handleDisableServer}
        onRefresh={handleRefreshDiscovery}
        onToggleTool={toggleToolSelection}
        onClearSelection={() => setSelectedToolIds([])}
        onAddToToolset={handleAddToToolset}
        onCredentialFormChange={setCredentialForm}
        onCredentialSubmit={handleCreateCredentialBinding}
        onCredentialRevoke={handleRevokeCredentialBinding}
      />
    </div>
  )
}

function ServerTable({
  servers,
  actionPending,
  refreshStatus,
  onSelectServer,
  onEdit,
  onDisable,
  onRefresh,
}: {
  servers: McpServerView[]
  actionPending: boolean
  refreshStatus: string | null
  onSelectServer: (serverId: string) => void
  onEdit: (server: McpServerView) => void
  onDisable: (server: McpServerView) => void
  onRefresh: (server: McpServerView) => void
}) {
  if (servers.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No MCP servers</EmptyTitle>
          <EmptyDescription>Add a catalog server or create a custom one.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <Table data-testid="mcp-server-list">
      <TableHeader>
        <TableRow>
          <TableHead className="w-10" />
          <TableHead>Server</TableHead>
          <TableHead>URL</TableHead>
          <TableHead>Auth type</TableHead>
          <TableHead>Status</TableHead>
          <TableHead className="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {servers.map((server) => (
          <TableRow key={server.id}>
            <TableCell>
              <div className="flex size-8 items-center justify-center rounded-md bg-muted text-muted-foreground">
                <AppIcon icon={McpServerIcon} size={18} stroke={1.5} aria-hidden />
              </div>
            </TableCell>
            <TableCell>
              <button
                type="button"
                aria-label={`Open ${server.display_name}`}
                className="flex min-w-0 flex-col gap-1 text-left"
                onClick={() => onSelectServer(server.id)}
              >
                <span className="truncate font-medium">{server.display_name}</span>
                <span className="truncate font-mono text-xs text-muted-foreground">
                  {server.server_key}
                </span>
              </button>
            </TableCell>
            <TableCell className="max-w-[20rem] truncate font-mono text-xs text-muted-foreground">
              {server.server_url}
            </TableCell>
            <TableCell>{formatAuthMode(server.auth_mode)}</TableCell>
            <TableCell>
              <ServerStatusBadge status={server.status} />
            </TableCell>
            <TableCell>
              <div className="flex justify-end gap-1">
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`Refresh ${server.display_name}`}
                  onClick={() => onRefresh(server)}
                  disabled={actionPending || server.status !== 'active'}
                >
                  <AppIcon icon={RefreshIcon} stroke={1.5} aria-hidden />
                </Button>
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`Edit ${server.display_name}`}
                  onClick={() => onEdit(server)}
                >
                  <AppIcon icon={Edit02Icon} stroke={1.5} aria-hidden />
                </Button>
                <Button
                  type="button"
                  size="icon-sm"
                  variant="destructive"
                  aria-label={`Delete ${server.display_name}`}
                  onClick={() => onDisable(server)}
                  disabled={actionPending || server.status !== 'active'}
                >
                  <AppIcon icon={Delete02Icon} stroke={1.5} aria-hidden />
                </Button>
              </div>
              {refreshStatus === 'pending' ? (
                <span className="sr-only">Discovery refresh pending</span>
              ) : null}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}

function ServerDetailDialog({
  server,
  section,
  form,
  tools,
  toolsPending,
  toolsError,
  selectedToolIds,
  credentialBindings,
  credentialBindingsPending,
  credentialBindingsError,
  credentialForm,
  refreshStatus,
  refreshErrorSummary,
  actionPending,
  onOpenChange,
  onSectionChange,
  onFormChange,
  onSubmit,
  onEdit,
  onDisable,
  onRefresh,
  onToggleTool,
  onClearSelection,
  onAddToToolset,
  onCredentialFormChange,
  onCredentialSubmit,
  onCredentialRevoke,
}: {
  server: McpServerView | null
  section: ServerSection
  form: ServerFormState
  tools: McpToolView[]
  toolsPending: boolean
  toolsError: string | null
  selectedToolIds: string[]
  credentialBindings: McpCredentialBindingView[]
  credentialBindingsPending: boolean
  credentialBindingsError: string | null
  credentialForm: CredentialBindingFormState
  refreshStatus: string | null
  refreshErrorSummary: string | null
  actionPending: boolean
  onOpenChange: (open: boolean) => void
  onSectionChange: (section: ServerSection) => void
  onFormChange: (form: ServerFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onEdit: (server: McpServerView) => void
  onDisable: (server: McpServerView) => void
  onRefresh: (server: McpServerView) => void
  onToggleTool: (toolId: string) => void
  onClearSelection: () => void
  onAddToToolset: () => void
  onCredentialFormChange: (form: CredentialBindingFormState) => void
  onCredentialSubmit: (event: FormEvent<HTMLFormElement>) => void
  onCredentialRevoke: (binding: McpCredentialBindingView) => void
}) {
  function setSection(nextSection: ServerSection) {
    if (nextSection === 'configuration' && server) {
      onFormChange(formFromServer(server))
    }
    onSectionChange(nextSection)
  }

  return (
    <Dialog open={Boolean(server)} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="overflow-hidden p-0 md:max-h-[720px] md:max-w-[980px]"
      >
        <DialogTitle className="sr-only">Manage MCP server</DialogTitle>
        <DialogDescription className="sr-only">
          Review server discovery status, tools, and credentials.
        </DialogDescription>

        {server ? (
          <SidebarProvider
            className="min-h-0 items-start"
            style={{ '--sidebar-width': '14rem' } as CSSProperties}
          >
            <Sidebar
              collapsible="none"
              className="hidden border-r border-[color:var(--color-border)] md:flex"
            >
              <SidebarContent className="p-3">
                <SidebarGroup className="px-0 py-0">
                  <SidebarGroupContent>
                    <SidebarMenu className="gap-1">
                      {serverSections.map((entry) => (
                        <SidebarMenuItem key={entry.value}>
                          <SidebarMenuButton
                            type="button"
                            className="h-10 px-3 py-2"
                            isActive={section === entry.value}
                            onClick={() => setSection(entry.value)}
                          >
                            <AppIcon icon={entry.icon} stroke={1.5} aria-hidden />
                            <span>{entry.label}</span>
                          </SidebarMenuButton>
                        </SidebarMenuItem>
                      ))}
                    </SidebarMenu>
                  </SidebarGroupContent>
                </SidebarGroup>
              </SidebarContent>
            </Sidebar>

            <main className="flex max-h-[720px] min-h-[560px] flex-1 flex-col overflow-hidden">
              <header className="flex shrink-0 flex-col gap-4 border-b border-[color:var(--color-border)] px-6 py-5">
                <div className="flex items-start gap-3">
                  <div className="flex size-11 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground">
                    <AppIcon icon={McpServerIcon} size={22} stroke={1.5} aria-hidden />
                  </div>
                  <div className="min-w-0 flex-1 pt-0.5">
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <h2 className="truncate text-lg leading-tight font-semibold text-[var(--color-text)]">
                        {server.display_name}
                      </h2>
                      <ServerStatusBadge status={server.status} />
                      <DiscoveryStatusBadge status={server.last_discovery_status} />
                    </div>
                    <p className="mt-1 truncate font-mono text-sm text-[var(--color-text-muted)]">
                      /mcp/{server.server_key}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      type="button"
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Refresh ${server.display_name}`}
                      onClick={() => onRefresh(server)}
                      disabled={actionPending || server.status !== 'active'}
                    >
                      <AppIcon icon={RefreshIcon} stroke={1.5} aria-hidden />
                    </Button>
                    <Button
                      type="button"
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Edit ${server.display_name}`}
                      onClick={() => onEdit(server)}
                    >
                      <AppIcon icon={Edit02Icon} stroke={1.5} aria-hidden />
                    </Button>
                    <Button
                      type="button"
                      size="icon-sm"
                      variant="destructive"
                      aria-label={`Delete ${server.display_name}`}
                      onClick={() => onDisable(server)}
                      disabled={actionPending || server.status !== 'active'}
                    >
                      <AppIcon icon={Delete02Icon} stroke={1.5} aria-hidden />
                    </Button>
                    <DialogClose asChild>
                      <Button type="button" variant="ghost" size="icon-sm" aria-label="Close">
                        <AppIcon icon={Cancel01Icon} stroke={1.5} aria-hidden />
                      </Button>
                    </DialogClose>
                  </div>
                </div>

                <div className="flex gap-2 overflow-x-auto md:hidden">
                  {serverSections.map((entry) => (
                    <Button
                      key={entry.value}
                      type="button"
                      size="sm"
                      variant={section === entry.value ? 'secondary' : 'ghost'}
                      onClick={() => setSection(entry.value)}
                    >
                      <AppIcon icon={entry.icon} stroke={1.5} aria-hidden data-icon="inline-start" />
                      {entry.label}
                    </Button>
                  ))}
                </div>
              </header>

              <div className="min-h-0 flex-1 overflow-y-auto p-6" data-testid="mcp-server-detail">
                {section === 'overview' ? (
                  <ServerOverviewPanel
                    server={server}
                    refreshStatus={refreshStatus}
                    refreshErrorSummary={refreshErrorSummary}
                  />
                ) : null}
                {section === 'configuration' ? (
                  <form className="flex min-h-full flex-col gap-6" onSubmit={onSubmit}>
                    <div className="flex flex-col gap-2">
                      <h3 className="text-sm font-semibold text-[var(--color-text)]">
                        Configuration
                      </h3>
                      <p className="text-sm text-[var(--color-text-muted)]">
                        Update endpoint, auth mode, timeout, and gateway auth configuration.
                      </p>
                    </div>
                    <ServerFormFields mode="edit" form={form} onFormChange={onFormChange} />
                    <DialogFooter className="mt-auto border-t border-[color:var(--color-border)] pt-4">
                      <Button type="button" variant="secondary" onClick={() => setSection('overview')}>
                        Cancel
                      </Button>
                      <Button type="submit" disabled={actionPending}>
                        {actionPending ? 'Saving...' : 'Save changes'}
                      </Button>
                    </DialogFooter>
                  </form>
                ) : null}
                {section === 'tools' ? (
                  <ServerToolsPanel
                    tools={tools}
                    toolsPending={toolsPending}
                    toolsError={toolsError}
                    selectedToolIds={selectedToolIds}
                    onToggleTool={onToggleTool}
                    onClearSelection={onClearSelection}
                    onAddToToolset={onAddToToolset}
                  />
                ) : null}
                {section === 'credentials' ? (
                  <CredentialBindingsPanel
                    bindings={credentialBindings}
                    form={credentialForm}
                    pending={actionPending || credentialBindingsPending}
                    error={credentialBindingsError}
                    onFormChange={onCredentialFormChange}
                    onSubmit={onCredentialSubmit}
                    onRevoke={onCredentialRevoke}
                  />
                ) : null}
              </div>
            </main>
          </SidebarProvider>
        ) : null}
      </DialogContent>
    </Dialog>
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
    <Card size="sm">
      <CardHeader>
        <CardTitle>Recommended catalog</CardTitle>
        <CardDescription>Import common MCP endpoints or customize before registration.</CardDescription>
      </CardHeader>
      <CardContent>
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
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => onCustomize(server)}
                >
                  Customize
                </Button>
                <Button type="button" size="sm" onClick={() => onImport(server)} disabled={pending}>
                  Import
                </Button>
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}

function formatAuthMode(authMode: string) {
  return authMode.replaceAll('_', ' ')
}
