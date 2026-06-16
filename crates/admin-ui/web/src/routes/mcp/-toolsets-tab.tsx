import { useEffect, useRef, useState, useTransition, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  addMcpToolset,
  disableExternalMcpToolset,
  saveMcpToolset,
  saveMcpToolsetTools,
} from '@/server/admin-data.functions'
import type { McpServerView, McpToolsetView } from '@/types/api'
import { useToolCatalog } from './-catalog'
import { MultiToolPicker, SelectedToolChips, type ToolGroup } from './-tool-picker'
import { MasterDetailShell, cnListButton } from './-shell'

export function ToolsetsTab({
  toolsets,
  servers,
  selectedToolsetId,
  onSelectToolset,
  seedToolIds,
  onSeedConsumed,
}: {
  toolsets: McpToolsetView[]
  servers: McpServerView[]
  selectedToolsetId: string | null
  onSelectToolset: (toolsetId: string | null) => void
  seedToolIds: string[]
  onSeedConsumed: () => void
}) {
  const router = useRouter()
  const catalog = useToolCatalog(servers)
  const [isPending, startTransition] = useTransition()
  const [createOpen, setCreateOpen] = useState(false)
  const seedAppliedRef = useRef(false)
  const [createForm, setCreateForm] = useState({
    toolset_key: '',
    display_name: '',
    description: '',
  })
  const [editForm, setEditForm] = useState({ display_name: '', description: '' })
  const [memberIds, setMemberIds] = useState<string[]>(() => seedToolIds)

  const selectedToolset = toolsets.find((toolset) => toolset.id === selectedToolsetId) ?? null
  const activeCount = toolsets.filter((toolset) => toolset.status === 'active').length

  const toolGroups: ToolGroup[] = servers
    .filter((server) => server.status === 'active')
    .map((server) => ({
      serverId: server.id,
      serverName: server.display_name,
      tools: catalog.byServer.get(server.id) ?? [],
    }))
    .filter((group) => group.tools.length > 0)

  useEffect(() => {
    if (seedToolIds.length > 0) {
      seedAppliedRef.current = true
      setMemberIds(seedToolIds)
      onSeedConsumed()
    }
    // Apply the Servers-tab hand-off once on mount; tabs remount on switch.
  }, [])

  useEffect(() => {
    if (seedAppliedRef.current) {
      seedAppliedRef.current = false
      return
    }
    setMemberIds([])
  }, [selectedToolset?.id])

  useEffect(() => {
    if (selectedToolset) {
      setEditForm({
        display_name: selectedToolset.display_name,
        description: selectedToolset.description ?? '',
      })
    }
  }, [selectedToolset])

  function handleCreateToolset(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const input = {
      toolset_key: createForm.toolset_key.trim(),
      display_name: createForm.display_name.trim(),
      description: emptyToNull(createForm.description),
    }
    if (!input.toolset_key || !input.display_name) {
      toast.error('Toolset key and display name are required')
      return
    }
    startTransition(async () => {
      try {
        const created = await addMcpToolset({ data: input })
        toast.success('MCP toolset created')
        setCreateForm({ toolset_key: '', display_name: '', description: '' })
        setCreateOpen(false)
        await router.invalidate()
        onSelectToolset(created.data.toolset.id)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to create MCP toolset')
      }
    })
  }

  function handleUpdateToolset(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedToolset) {
      return
    }
    const input = {
      display_name: editForm.display_name.trim(),
      description: emptyToNull(editForm.description),
    }
    if (!input.display_name) {
      toast.error('Display name is required')
      return
    }
    startTransition(async () => {
      try {
        await saveMcpToolset({ data: { toolsetId: selectedToolset.id, input } })
        toast.success('MCP toolset updated')
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to update MCP toolset')
      }
    })
  }

  function handleDisableToolset(toolset: McpToolsetView) {
    startTransition(async () => {
      try {
        await disableExternalMcpToolset({ data: { toolsetId: toolset.id } })
        toast.success('MCP toolset disabled')
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to disable MCP toolset')
      }
    })
  }

  function handleReplaceMembership() {
    if (!selectedToolset) {
      return
    }
    startTransition(async () => {
      try {
        const response = await saveMcpToolsetTools({
          data: { toolsetId: selectedToolset.id, toolIds: memberIds },
        })
        const count = response.data.tool_ids.length
        setMemberIds(response.data.tool_ids)
        toast.success(`Membership replaced (${count} tool${count === 1 ? '' : 's'})`)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to replace toolset membership')
      }
    })
  }

  function toggleMember(toolId: string, checked: boolean) {
    setMemberIds((current) =>
      checked ? [...current, toolId] : current.filter((id) => id !== toolId),
    )
  }

  const detail = selectedToolset ? (
    <div className="flex min-w-0 flex-col gap-4" data-testid="mcp-toolset-detail">
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="truncate text-lg font-semibold">{selectedToolset.display_name}</h2>
            <Badge variant={selectedToolset.status === 'active' ? 'default' : 'secondary'}>
              {selectedToolset.status}
            </Badge>
          </div>
          <div className="mt-1 truncate font-mono text-xs text-[var(--color-text-muted)]">
            {selectedToolset.toolset_key}
          </div>
        </div>
        <Button
          type="button"
          variant="destructive"
          disabled={isPending || selectedToolset.status !== 'active'}
          onClick={() => handleDisableToolset(selectedToolset)}
        >
          Disable
        </Button>
      </div>

      <form className="rounded-md border p-4" onSubmit={handleUpdateToolset}>
        <FieldGroup className="grid gap-3 md:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="toolset-display-name">Display name</FieldLabel>
            <Input
              id="toolset-display-name"
              value={editForm.display_name}
              onChange={(event) =>
                setEditForm((current) => ({ ...current, display_name: event.target.value }))
              }
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="toolset-description">Description</FieldLabel>
            <Input
              id="toolset-description"
              value={editForm.description}
              onChange={(event) =>
                setEditForm((current) => ({ ...current, description: event.target.value }))
              }
            />
          </Field>
        </FieldGroup>
        <div className="mt-3 flex justify-end">
          <Button type="submit" variant="outline" disabled={isPending}>
            Save details
          </Button>
        </div>
      </form>

      <div className="flex min-w-0 flex-col gap-3 rounded-md border p-4">
        <div className="flex flex-col gap-1">
          <h3 className="font-medium">Membership</h3>
          <p className="text-sm text-[var(--color-text-muted)]">
            Pick tools from the live discovery catalog to bundle into this toolset.
          </p>
        </div>

        <Alert>
          <AlertTitle>Membership is write-only</AlertTitle>
          <AlertDescription>
            The gateway has no endpoint to read a toolset&apos;s current tools, so existing members
            can&apos;t be pre-loaded. Saving <strong>replaces</strong> the full membership with your
            current selection.
          </AlertDescription>
        </Alert>

        {catalog.error ? (
          <Alert variant="destructive">
            <AlertTitle>Catalog failed to load</AlertTitle>
            <AlertDescription>{catalog.error}</AlertDescription>
          </Alert>
        ) : null}

        <MultiToolPicker
          groups={toolGroups}
          selectedIds={memberIds}
          onToggle={toggleMember}
          disabled={catalog.pending}
          buttonLabel={catalog.pending ? 'Loading tools…' : 'Select tools'}
        />
        <SelectedToolChips
          toolIds={memberIds}
          byId={catalog.byId}
          onRemove={(toolId) => toggleMember(toolId, false)}
        />

        <div className="flex items-center justify-between gap-2">
          <span className="text-sm text-[var(--color-text-muted)]">
            {memberIds.length} tool{memberIds.length === 1 ? '' : 's'} selected
          </span>
          <Button type="button" onClick={handleReplaceMembership} disabled={isPending}>
            Replace membership
          </Button>
        </div>
      </div>
    </div>
  ) : (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>Select a toolset</EmptyTitle>
        <EmptyDescription>Toolset details and membership appear here.</EmptyDescription>
      </EmptyHeader>
    </Empty>
  )

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-[var(--color-text-muted)]">
          {toolsets.length} toolset{toolsets.length === 1 ? '' : 's'} · {activeCount} active
        </p>
        <Button type="button" onClick={() => setCreateOpen(true)}>
          New toolset
        </Button>
      </div>

      {seedToolIds.length > 0 ? (
        <Alert>
          <AlertTitle>{seedToolIds.length} tools carried over</AlertTitle>
          <AlertDescription>
            Select or create a toolset, then save to apply the tools you picked on the Servers tab.
          </AlertDescription>
        </Alert>
      ) : null}

      <MasterDetailShell
        detailOpen={Boolean(selectedToolset)}
        onDetailOpenChange={(open) => {
          if (!open) {
            onSelectToolset(null)
          }
        }}
        detailTitle={selectedToolset?.display_name ?? 'Toolset detail'}
        detailDescription="Edit details and tool membership."
        list={
          <ToolsetList
            toolsets={toolsets}
            selectedToolsetId={selectedToolset?.id ?? null}
            onSelectToolset={onSelectToolset}
          />
        }
        detail={detail}
      />

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New toolset</DialogTitle>
            <DialogDescription>Named bundles of active MCP tools.</DialogDescription>
          </DialogHeader>
          <form className="flex flex-col gap-4" onSubmit={handleCreateToolset}>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="new-toolset-key">Key</FieldLabel>
                <Input
                  id="new-toolset-key"
                  value={createForm.toolset_key}
                  onChange={(event) =>
                    setCreateForm((current) => ({ ...current, toolset_key: event.target.value }))
                  }
                  placeholder="github-readonly"
                  required
                />
                <FieldDescription>Stable identifier used by grants and policy.</FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="new-toolset-name">Display name</FieldLabel>
                <Input
                  id="new-toolset-name"
                  value={createForm.display_name}
                  onChange={(event) =>
                    setCreateForm((current) => ({ ...current, display_name: event.target.value }))
                  }
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="new-toolset-description">Description</FieldLabel>
                <Textarea
                  id="new-toolset-description"
                  value={createForm.description}
                  onChange={(event) =>
                    setCreateForm((current) => ({ ...current, description: event.target.value }))
                  }
                  rows={2}
                />
              </Field>
            </FieldGroup>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setCreateOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={isPending}>
                {isPending ? 'Creating…' : 'Create toolset'}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function ToolsetList({
  toolsets,
  selectedToolsetId,
  onSelectToolset,
}: {
  toolsets: McpToolsetView[]
  selectedToolsetId: string | null
  onSelectToolset: (toolsetId: string) => void
}) {
  if (toolsets.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No toolsets</EmptyTitle>
          <EmptyDescription>Create a toolset to bundle tools for access grants.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <div className="flex min-w-0 flex-col gap-2" data-testid="mcp-toolset-list">
      {toolsets.map((toolset) => (
        <button
          key={toolset.id}
          type="button"
          className={cnListButton(selectedToolsetId === toolset.id)}
          onClick={() => onSelectToolset(toolset.id)}
        >
          <div className="flex min-w-0 items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="truncate font-medium">{toolset.display_name}</div>
              <div className="truncate font-mono text-xs text-[var(--color-text-muted)]">
                {toolset.toolset_key}
              </div>
            </div>
            <Badge variant={toolset.status === 'active' ? 'default' : 'secondary'}>
              {toolset.status}
            </Badge>
          </div>
        </button>
      ))}
    </div>
  )
}

function emptyToNull(value: string) {
  const trimmed = value.trim()
  return trimmed ? trimmed : null
}
