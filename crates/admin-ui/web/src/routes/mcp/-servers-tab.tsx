import {
  useEffect,
  useRef,
  useState,
  useTransition,
  type CSSProperties,
  type FormEvent,
} from 'react'
import {
  CheckListIcon,
  Configuration01Icon,
  ShieldKeyIcon,
  ToolsIcon,
  ViewIcon,
} from '@hugeicons/core-free-icons'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { SectionNavSidebar, SectionNavTabs } from '@/components/shared/section-nav'
import { McpServerIconMark } from '@/components/mcp/mcp-server-mark'
import {
  describeDiscoveryResult,
  needsAttention,
  type DiscoveryRefreshResult,
} from '@/components/mcp/server-discovery'
import { ServerRegistry } from '@/components/mcp/server-registry'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from '@/components/ui/dialog'
import { SidebarProvider } from '@/components/ui/sidebar'
import {
  addMcpServer,
  disableExternalMcpServer,
  enableExternalMcpServer,
  getMcpCredentialBindings,
  getMcpServerTools,
  refreshExternalMcpServerDiscovery,
  removeMcpCredentialBinding,
  saveMcpCredentialBinding,
  saveMcpServer,
  transferExternalMcpServerOwnership,
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
  ServerHeaderActions,
  ServerOverviewPanel,
  ServerOwnershipTransfer,
  ServerStatusBadge,
  ServerToolsPanel,
} from './-components'
import { platformMcpScope, type McpWorkspaceScope } from './-scope'
import { ToolReviewsPanel } from './-tool-reviews'
import {
  emptyCredentialBindingForm,
  emptyServerForm,
  formFromRecommended,
  formFromServer,
  formToCredentialBindingInput,
  formToCreateInput,
  formToUpdateInput,
  type CredentialBindingFormState,
  type ServerFormState,
} from './-server-forms'

type ServerSection = 'overview' | 'configuration' | 'tools' | 'reviews' | 'credentials'

const serverSections = [
  { id: 'overview', label: 'Overview', icon: ViewIcon },
  { id: 'configuration', label: 'Configuration', icon: Configuration01Icon },
  { id: 'tools', label: 'Tools', icon: ToolsIcon },
  { id: 'reviews', label: 'Reviews', icon: CheckListIcon },
  { id: 'credentials', label: 'Credentials', icon: ShieldKeyIcon },
] as const

// Servers the caller cannot manage are read-only: no configuration or credentials.
const readOnlySections = new Set<ServerSection>(['overview', 'tools'])
// Tool reviews and tool policy are platform-managed, like tag rules.
const platformSections = new Set<ServerSection>(['reviews'])

// Server selection coordinates CRUD, discovery, tools, and credentials through one lifecycle.
// oxlint-disable-next-line eslint/max-lines-per-function
export function ServersTab({
  servers,
  recommended,
  selectedServerId,
  scope = platformMcpScope,
  onSelectServer,
  onAddToToolset,
}: {
  servers: McpServerView[]
  recommended: RecommendedMcpServerView[]
  selectedServerId: string | null
  scope?: McpWorkspaceScope
  onSelectServer: (serverId: string | null) => void
  onAddToToolset: (toolIds: string[]) => void
}) {
  const newServerVisibility = scope.platformAdmin ? 'organisation' : 'team'
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
    emptyCredentialBindingForm(scope.teamId),
  )
  const [refreshingServerIds, setRefreshingServerIds] = useState<string[]>([])
  const [refreshResults, setRefreshResults] = useState<
    Record<string, DiscoveryRefreshResult & { revision: number }>
  >({})
  const [catalogOpen, setCatalogOpen] = useState(false)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editingServer, setEditingServer] = useState<McpServerView | null>(null)
  const [formState, setFormState] = useState<ServerFormState>(() => emptyServerForm())

  const selectedServer = servers.find((server) => server.id === selectedServerId) ?? null
  const selectedRefresh = selectedServer ? refreshResults[selectedServer.id] : null
  const selectedRefreshing = Boolean(
    selectedServer && refreshingServerIds.includes(selectedServer.id),
  )
  // Refresh responses omit inactive tools; reload the complete list after each result.
  const discoveryRevision = selectedRefresh?.revision ?? 0
  // Read when a refresh finishes, not when it starts: the admin may have moved on meanwhile.
  const visibleOverviewIdRef = useRef<string | null>(null)

  useEffect(() => {
    visibleOverviewIdRef.current = section === 'overview' ? (selectedServer?.id ?? null) : null
  }, [section, selectedServer])

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
  }, [selectedServer, discoveryRevision])

  useEffect(() => {
    // Credentials of a server the caller cannot manage are not theirs to see.
    if (!selectedServer?.can_manage) {
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
    setSection('overview')
    onSelectServer(serverId)
  }

  async function selectServerAfterMutation(serverId: string) {
    setRefreshResults((current) => {
      const next = { ...current }
      delete next[serverId]
      return next
    })
    await router.invalidate()
    onSelectServer(serverId)
  }

  function openCreateDialog(server?: RecommendedMcpServerView) {
    setFormState(
      server
        ? formFromRecommended(server, newServerVisibility)
        : emptyServerForm(newServerVisibility),
    )
    setCatalogOpen(false)
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
        setCatalogOpen(false)
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

  function handleEnableServer(server: McpServerView) {
    startTransition(async () => {
      try {
        const response = await enableExternalMcpServer({ data: { serverId: server.id } })
        toast.success('MCP server enabled')
        await selectServerAfterMutation(response.data.server.id)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to enable MCP server')
      }
    })
  }

  // An earlier result stays on screen until the new one replaces it.
  async function handleRefreshDiscovery(server: McpServerView) {
    if (refreshingServerIds.includes(server.id)) return
    setRefreshingServerIds((current) => [...current, server.id])
    try {
      const response = await refreshExternalMcpServerDiscovery({ data: { serverId: server.id } })
      const result = {
        status: response.data.status,
        errorKind: response.data.error_kind ?? null,
        httpStatus: response.data.error_http_status ?? null,
        errorSummary: response.data.error_summary ?? null,
        toolCount: response.data.tools.length,
      }
      setRefreshResults((current) => ({
        ...current,
        [server.id]: { ...result, revision: (current[server.id]?.revision ?? 0) + 1 },
      }))
      announceRefreshResult(server, result)
      await router.invalidate()
    } catch (error) {
      const result = {
        status: 'request_failed',
        errorSummary: error instanceof Error ? error.message : null,
        toolCount: 0,
      }
      setRefreshResults((current) => ({
        ...current,
        [server.id]: { ...result, revision: current[server.id]?.revision ?? 0 },
      }))
      announceRefreshResult(server, result)
    } finally {
      setRefreshingServerIds((current) => current.filter((id) => id !== server.id))
    }
  }

  // The overview explains every result. A brief toast reports one only while that overview is
  // hidden, and links back to it.
  function announceRefreshResult(server: McpServerView, result: DiscoveryRefreshResult) {
    if (visibleOverviewIdRef.current === server.id) return
    const message = `${describeDiscoveryResult(result, server).title} for ${server.display_name}`
    if (result.status === 'success') {
      toast.success(message)
      return
    }
    toast.error(message, {
      action: { label: 'View', onClick: () => handleSelectServer(server.id) },
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
        setCredentialForm(emptyCredentialBindingForm(scope.teamId))
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

  function handleTransferOwnership(server: McpServerView, ownerTeamId: string | null) {
    startTransition(async () => {
      try {
        const response = await transferExternalMcpServerOwnership({
          data: { serverId: server.id, input: { owner_team_id: ownerTeamId } },
        })
        toast.success('Ownership transferred')
        await selectServerAfterMutation(response.data.server.id)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to transfer ownership')
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
      <ServerRegistry
        servers={servers}
        actionPending={isPending}
        refreshingServerIds={refreshingServerIds}
        onManage={handleSelectServer}
        onRefresh={handleRefreshDiscovery}
        onAdd={scope.readOnly ? undefined : () => openCreateDialog()}
        onCatalog={scope.readOnly ? undefined : () => setCatalogOpen(true)}
      />

      <Dialog open={catalogOpen} onOpenChange={setCatalogOpen}>
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-3xl">
          <DialogTitle>Browse MCP catalog</DialogTitle>
          <DialogDescription>
            Import a recommended endpoint, or customize its settings before registration.
          </DialogDescription>
          <RecommendedCatalog
            recommended={recommended}
            servers={servers}
            pending={isPending}
            onCustomize={openCreateDialog}
            onImport={handleImportRecommended}
          />
        </DialogContent>
      </Dialog>

      <ServerFormDialog
        mode="create"
        open={createDialogOpen}
        pending={isPending}
        form={formState}
        ownership={{
          canPublish: scope.canPublish,
          platformOwned: scope.platformAdmin,
          teams: scope.platformAdmin ? scope.teams : undefined,
          canRequireApproval: scope.platformAdmin,
        }}
        onOpenChange={setCreateDialogOpen}
        onFormChange={setFormState}
        onSubmit={handleCreateServer}
      />

      <ServerDetailDialog
        server={selectedServer}
        scope={scope}
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
        refreshResult={selectedRefresh ?? null}
        refreshing={selectedRefreshing}
        actionPending={isPending || selectedRefreshing}
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
        onEnable={handleEnableServer}
        onRefresh={handleRefreshDiscovery}
        onToggleTool={toggleToolSelection}
        onClearSelection={() => setSelectedToolIds([])}
        // Tool sets are organisation-wide, so only organisation servers' tools join them.
        onAddToToolset={
          scope.platformAdmin && selectedServer?.visibility === 'organisation'
            ? handleAddToToolset
            : undefined
        }
        onToolUpdated={
          scope.platformAdmin
            ? (updated) =>
                setTools((current) =>
                  current.map((tool) => (tool.id === updated.id ? updated : tool)),
                )
            : undefined
        }
        onTransfer={handleTransferOwnership}
        onCredentialFormChange={setCredentialForm}
        onCredentialSubmit={handleCreateCredentialBinding}
        onCredentialRevoke={handleRevokeCredentialBinding}
      />
    </div>
  )
}

// Keep responsive navigation, header actions, and section routing in one dialog shell.
// oxlint-disable-next-line eslint/max-lines-per-function
function ServerDetailDialog({
  server,
  scope,
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
  refreshResult,
  refreshing,
  actionPending,
  onOpenChange,
  onSectionChange,
  onFormChange,
  onSubmit,
  onEdit,
  onDisable,
  onEnable,
  onRefresh,
  onToggleTool,
  onClearSelection,
  onAddToToolset,
  onToolUpdated,
  onTransfer,
  onCredentialFormChange,
  onCredentialSubmit,
  onCredentialRevoke,
}: {
  server: McpServerView | null
  scope: McpWorkspaceScope
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
  refreshResult: DiscoveryRefreshResult | null
  refreshing: boolean
  actionPending: boolean
  onOpenChange: (open: boolean) => void
  onSectionChange: (section: ServerSection) => void
  onFormChange: (form: ServerFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onEdit: (server: McpServerView) => void
  onDisable: (server: McpServerView) => void
  onEnable: (server: McpServerView) => void
  onRefresh: (server: McpServerView) => void
  onToggleTool: (toolId: string) => void
  onClearSelection: () => void
  onAddToToolset?: () => void
  onToolUpdated?: (tool: McpToolView) => void
  onTransfer: (server: McpServerView, ownerTeamId: string | null) => void
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

  const { canManage, sections, visibleSection } = useServerSections(
    server,
    section,
    scope.platformAdmin,
  )

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
            className="min-h-0 max-w-full min-w-0 items-start overflow-hidden"
            style={{ '--sidebar-width': '14rem' } as CSSProperties}
          >
            <SectionNavSidebar
              sections={sections}
              activeSection={visibleSection}
              onSectionChange={setSection}
            />

            <main className="flex max-h-[720px] min-h-[560px] min-w-0 flex-1 flex-col overflow-hidden">
              <header className="border-border flex shrink-0 flex-col gap-4 border-b px-6 py-5">
                <div className="flex items-start gap-3">
                  <div className="bg-primary text-primary-foreground flex size-11 shrink-0 items-center justify-center rounded-full">
                    <McpServerIconMark server={server} size={22} bare />
                  </div>
                  <div className="min-w-0 flex-1 pt-0.5">
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <h2 className="text-foreground truncate text-lg leading-tight font-semibold">
                        {server.display_name}
                      </h2>
                      <ServerStatusBadge status={server.status} />
                      <DiscoveryStatusBadge
                        status={server.last_discovery_status}
                        attention={needsAttention(server)}
                      />
                    </div>
                    <p className="text-subtle-foreground mt-1 truncate font-mono text-sm">
                      /mcp/{server.server_key}
                    </p>
                  </div>
                  <ServerHeaderActions
                    server={server}
                    canManage={canManage}
                    actionPending={actionPending}
                    onRefresh={onRefresh}
                    onEdit={onEdit}
                    onDisable={onDisable}
                    onEnable={onEnable}
                  />
                </div>

                <SectionNavTabs
                  sections={sections}
                  activeSection={visibleSection}
                  onSectionChange={setSection}
                />
              </header>

              <div
                className="min-h-0 min-w-0 flex-1 overflow-y-auto p-6"
                data-testid="mcp-server-detail"
              >
                {visibleSection === 'overview' ? (
                  <ServerOverviewPanel
                    server={server}
                    refreshResult={refreshResult}
                    refreshing={refreshing}
                    retryDisabled={actionPending || !canManage || server.status !== 'active'}
                    onRetry={() => onRefresh(server)}
                    ownershipControl={
                      <PlatformOwnershipControl
                        server={server}
                        scope={scope}
                        pending={actionPending}
                        onTransfer={onTransfer}
                      />
                    }
                  />
                ) : null}
                {visibleSection === 'configuration' ? (
                  <ServerConfigurationForm
                    server={server}
                    scope={scope}
                    form={form}
                    pending={actionPending}
                    onFormChange={onFormChange}
                    onSubmit={onSubmit}
                    onCancel={() => setSection('overview')}
                  />
                ) : null}
                {visibleSection === 'tools' ? (
                  <ServerToolsPanel
                    tools={tools}
                    toolsPending={toolsPending}
                    toolsError={toolsError}
                    selectedToolIds={selectedToolIds}
                    onToggleTool={onToggleTool}
                    onClearSelection={onClearSelection}
                    onAddToToolset={onAddToToolset}
                    onToolUpdated={onToolUpdated}
                  />
                ) : null}
                {visibleSection === 'reviews' && onToolUpdated ? (
                  <ToolReviewsPanel
                    server={server}
                    tools={tools}
                    toolsPending={toolsPending}
                    toolsError={toolsError}
                    onToolUpdated={onToolUpdated}
                  />
                ) : null}
                {visibleSection === 'credentials' ? (
                  <CredentialBindingsPanel
                    bindings={credentialBindings}
                    form={credentialForm}
                    pending={actionPending || credentialBindingsPending}
                    error={credentialBindingsError}
                    teamScoped={!scope.platformAdmin}
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

/** Which detail sections the caller sees for `server`, and the one to show. */
function useServerSections(
  server: McpServerView | null,
  section: ServerSection,
  platformAdmin: boolean,
) {
  const canManage = server?.can_manage ?? false
  const shown = (value: ServerSection) =>
    (canManage || readOnlySections.has(value)) && (platformAdmin || !platformSections.has(value))
  const sections = serverSections.filter((entry) => shown(entry.id))
  const visibleSection: ServerSection = shown(section) ? section : 'overview'
  return { canManage, sections, visibleSection }
}

function ServerConfigurationForm({
  server,
  scope,
  form,
  pending,
  onFormChange,
  onSubmit,
  onCancel,
}: {
  server: McpServerView
  scope: McpWorkspaceScope
  form: ServerFormState
  pending: boolean
  onFormChange: (form: ServerFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onCancel: () => void
}) {
  return (
    <form className="flex min-h-full flex-col gap-6" onSubmit={onSubmit}>
      <div className="flex flex-col gap-2">
        <h3 className="text-foreground text-sm font-semibold">Configuration</h3>
        <p className="text-subtle-foreground text-sm">
          Update endpoint, auth mode, timeout, and gateway auth configuration.
        </p>
      </div>
      <ServerFormFields
        mode="edit"
        form={form}
        ownership={{
          canPublish: scope.canPublish,
          platformOwned: !server.owner_team_id,
          canRequireApproval: scope.platformAdmin,
        }}
        onFormChange={onFormChange}
      />
      <DialogFooter className="border-border mt-auto border-t pt-4">
        <Button type="button" variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" disabled={pending}>
          {pending ? 'Saving...' : 'Save changes'}
        </Button>
      </DialogFooter>
    </form>
  )
}

/** Platform admins move a server between teams from its overview. */
function PlatformOwnershipControl({
  server,
  scope,
  pending,
  onTransfer,
}: {
  server: McpServerView
  scope: McpWorkspaceScope
  pending: boolean
  onTransfer: (server: McpServerView, ownerTeamId: string | null) => void
}) {
  if (!scope.platformAdmin) return null
  return (
    <ServerOwnershipTransfer
      // Remount when the owner changes, so the selector resets after a transfer.
      key={`${server.id}:${server.owner_team_id ?? 'platform'}`}
      server={server}
      teams={scope.teams}
      pending={pending}
      onTransfer={(ownerTeamId) => onTransfer(server, ownerTeamId)}
    />
  )
}

export function RecommendedCatalog({
  recommended,
  servers,
  pending,
  onCustomize,
  onImport,
}: {
  recommended: RecommendedMcpServerView[]
  servers: McpServerView[]
  pending: boolean
  onCustomize: (server: RecommendedMcpServerView) => void
  onImport: (server: RecommendedMcpServerView) => void
}) {
  if (recommended.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No catalog servers available</EmptyTitle>
          <EmptyDescription>Use Add server to configure a custom endpoint.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Recommended catalog</CardTitle>
        <CardDescription>
          Import common MCP endpoints or customize before registration.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid gap-2">
          {recommended.map((server) => (
            <div
              key={server.catalog_key}
              className="flex min-w-0 items-center justify-between gap-2 rounded-md border p-3"
            >
              <div className="flex min-w-0 items-center gap-3">
                <McpServerIconMark server={server} />
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium">{server.display_name}</div>
                  <div className="text-subtle-foreground truncate text-xs">
                    {server.catalog_key}
                  </div>
                </div>
              </div>
              <div className="flex shrink-0 gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => onCustomize(server)}
                  disabled={pending}
                >
                  Customize
                </Button>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => onImport(server)}
                  disabled={
                    pending ||
                    servers.some((registered) => registered.server_key === server.catalog_key)
                  }
                >
                  {servers.some((registered) => registered.server_key === server.catalog_key)
                    ? 'Registered'
                    : 'Import'}
                </Button>
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
