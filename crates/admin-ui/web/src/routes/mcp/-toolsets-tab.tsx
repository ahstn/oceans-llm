import { useEffect, useEffectEvent, useState, useTransition, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { ToolsetWorkbench } from '@/components/mcp/toolset-workbench'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  addMcpToolset,
  disableExternalMcpToolset,
  getMcpConnectionInfo,
  saveMcpToolset,
} from '@/server/admin-data.functions'
import type { McpServerView, McpToolsetView } from '@/types/api'
import { useToolCatalog } from './-catalog'
import { useToolsetMemberships } from './-toolset-membership'

type ToolsetsTabProps = {
  toolsets: McpToolsetView[]
  servers: McpServerView[]
  selectedToolsetId: string | null
  onSelectToolset: (toolsetId: string | null) => void
  seedToolIds: string[]
  onSeedConsumed: () => void
}

async function loadConnectionInfo() {
  const response = await getMcpConnectionInfo()
  return response.data
}

export function ToolsetsTab(props: ToolsetsTabProps) {
  const { toolsets, servers, selectedToolsetId, onSelectToolset, seedToolIds } = props
  const router = useRouter()
  const catalog = useToolCatalog(servers, { includeInactive: true })
  const memberships = useToolsetMemberships(toolsets)
  const [busy, startTransition] = useTransition()
  const [detailsTarget, setDetailsTarget] = useState<McpToolsetView | 'new' | null>(null)
  const [clearTarget, setClearTarget] = useState<string | null>(null)
  const selected = toolsets.find((toolset) => toolset.id === selectedToolsetId) ?? null
  // A newly created set already has a draft while its loader data catches up.
  const knownSelection = Boolean(
    selected || (selectedToolsetId && memberships.entries[selectedToolsetId]),
  )
  const carriedCount = useImportedTools(
    { ...props, selectedToolsetId: knownSelection ? selectedToolsetId : null },
    memberships,
  )
  const selectDefault = useEffectEvent(() => onSelectToolset(toolsets[0]?.id ?? null))

  useEffect(() => {
    if (!knownSelection && toolsets.length > 0 && carriedCount === 0 && seedToolIds.length === 0) {
      selectDefault()
    }
  }, [knownSelection, toolsets.length, carriedCount, seedToolIds.length])

  async function saveMembers(id: string) {
    if (catalog.pending || catalog.error || busy) return
    try {
      const count = await memberships.save(id)
      if (count !== null) toast.success(`Tool set saved (${count} tool${count === 1 ? '' : 's'})`)
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to save tool set')
    }
  }

  function requestSave(id: string) {
    const entry = memberships.entries[id]
    if (!entry || !entry.dirty || entry.loading || entry.error || entry.saving) return
    if (entry.toolIds.length === 0 && entry.savedToolIds.length > 0) setClearTarget(id)
    else void saveMembers(id)
  }

  function disableSet(id: string) {
    startTransition(async () => {
      try {
        await disableExternalMcpToolset({ data: { toolsetId: id } })
        toast.success('Tool set disabled')
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to disable tool set')
      }
    })
  }

  function toggleTool(id: string, checked: boolean) {
    if (!selected || catalog.pending || catalog.error || busy) return
    const tool = catalog.byId.get(id)
    const activeServer = servers.some(
      (server) => server.id === tool?.server_id && server.status === 'active',
    )
    if (checked && (!tool?.is_active || !activeServer)) return
    memberships.update(selected.id, (ids) =>
      checked ? [...ids, id] : ids.filter((value) => value !== id),
    )
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      {carriedCount > 0 && (
        <Alert>
          <AlertTitle>
            {carriedCount} tool{carriedCount === 1 ? '' : 's'} carried over
          </AlertTitle>
          <AlertDescription>
            Select or create a tool set to add the tools you picked in Servers.
          </AlertDescription>
        </Alert>
      )}
      <ToolsetWorkbench
        loadConnectionInfo={loadConnectionInfo}
        toolsets={toolsets}
        servers={servers}
        tools={catalog.tools}
        selectedId={selected?.id ?? null}
        memberships={memberships.entries}
        catalogPending={catalog.pending}
        catalogError={catalog.error}
        onRetryCatalog={catalog.reload}
        onRetryMembership={memberships.reload}
        onSelect={onSelectToolset}
        onEdit={(id) => setDetailsTarget(toolsets.find((set) => set.id === id) ?? null)}
        onSave={requestSave}
        onCreate={() => setDetailsTarget('new')}
        onDisable={disableSet}
        onAccess={() => void router.navigate({ to: '/mcp', search: { tab: 'access' } })}
        onToggleTool={toggleTool}
        onRemoveUnavailable={(id) => toggleTool(id, false)}
        busy={busy}
      />
      {detailsTarget && (
        <ToolsetDetailsDialog
          key={detailsTarget === 'new' ? 'new' : detailsTarget.id}
          toolset={detailsTarget === 'new' ? null : detailsTarget}
          onClose={() => setDetailsTarget(null)}
          onCreated={(id) => {
            memberships.initialize(id)
            onSelectToolset(id)
          }}
        />
      )}
      <ClearToolsetDialog
        toolset={toolsets.find((set) => set.id === clearTarget) ?? null}
        onClose={() => setClearTarget(null)}
        onConfirm={() => {
          if (clearTarget) void saveMembers(clearTarget)
        }}
      />
    </div>
  )
}

function useImportedTools(
  props: ToolsetsTabProps,
  memberships: ReturnType<typeof useToolsetMemberships>,
) {
  const [pendingIds, setPendingIds] = useState(props.seedToolIds)
  // Pin this import to its first valid destination while that set's membership loads.
  // Deriving it from the current selection would move the import when the user switches sets.
  const [targetId, setTargetId] = useState<string | null>(null)
  const seedKey = props.seedToolIds.join(',')
  const entry = targetId ? memberships.entries[targetId] : undefined
  const canImport = Boolean(entry && !entry.loading && !entry.error && !entry.saving)
  const consume = useEffectEvent(() => {
    if (props.seedToolIds.length === 0) return
    setPendingIds((ids) => [...new Set([...ids, ...props.seedToolIds])])
    setTargetId(props.selectedToolsetId)
    // Remove the consumed route seed so a reload does not import it a second time.
    props.onSeedConsumed()
  })
  useEffect(() => {
    consume()
  }, [seedKey])
  useEffect(() => {
    if (pendingIds.length > 0 && !targetId && props.selectedToolsetId)
      setTargetId(props.selectedToolsetId)
  }, [pendingIds.length, targetId, props.selectedToolsetId])
  const merge = useEffectEvent(() => {
    if (!targetId || pendingIds.length === 0) return
    memberships.update(targetId, (ids) => [...ids, ...pendingIds])
    setPendingIds([])
    setTargetId(null)
  })
  useEffect(() => {
    if (canImport) merge()
  }, [targetId, pendingIds.length, canImport])
  return pendingIds.length
}

function ToolsetDetailsDialog({
  toolset,
  onClose,
  onCreated,
}: {
  toolset: McpToolsetView | null
  onClose: () => void
  onCreated: (id: string) => void
}) {
  const router = useRouter()
  const [pending, startTransition] = useTransition()
  const [form, setForm] = useState({
    toolset_key: toolset?.toolset_key ?? '',
    display_name: toolset?.display_name ?? '',
    description: toolset?.description ?? '',
  })
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const input = {
      display_name: form.display_name.trim(),
      description: form.description.trim() || null,
    }
    if (!input.display_name || (!toolset && !form.toolset_key.trim())) return
    startTransition(async () => {
      try {
        let createdId: string | null = null
        if (toolset) {
          await saveMcpToolset({ data: { toolsetId: toolset.id, input } })
        } else {
          const { data } = await addMcpToolset({
            data: { ...input, toolset_key: form.toolset_key.trim() },
          })
          createdId = data.toolset.id
        }
        await router.invalidate()
        if (createdId) onCreated(createdId)
        toast.success(toolset ? 'Tool set details saved' : 'Tool set created')
        onClose()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save tool set details')
      }
    })
  }
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) onClose()
      }}
    >
      <DialogContent className="min-w-0 overflow-hidden">
        <DialogHeader>
          <DialogTitle>{toolset ? 'Edit tool set' : 'New tool set'}</DialogTitle>
          <DialogDescription>
            {toolset
              ? 'Update the name and description for this tool set.'
              : 'Create a named collection of MCP tools.'}
          </DialogDescription>
        </DialogHeader>
        <form className="flex min-w-0 flex-col gap-4" onSubmit={submit}>
          <FieldGroup>
            {!toolset && (
              <Field>
                <FieldLabel htmlFor="toolset-key">Key</FieldLabel>
                <Input
                  id="toolset-key"
                  value={form.toolset_key}
                  onChange={(event) => setForm({ ...form, toolset_key: event.target.value })}
                  required
                  disabled={pending}
                />
                <FieldDescription>Stable identifier used by grants and policy.</FieldDescription>
              </Field>
            )}
            <Field>
              <FieldLabel htmlFor="toolset-name">Display name</FieldLabel>
              <Input
                id="toolset-name"
                value={form.display_name}
                onChange={(event) => setForm({ ...form, display_name: event.target.value })}
                required
                disabled={pending}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="toolset-description">Description</FieldLabel>
              <Textarea
                id="toolset-description"
                rows={3}
                value={form.description}
                onChange={(event) => setForm({ ...form, description: event.target.value })}
                disabled={pending}
              />
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={pending}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending}>
              {pending ? 'Saving…' : toolset ? 'Save details' : 'Create tool set'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function ClearToolsetDialog({
  toolset,
  onClose,
  onConfirm,
}: {
  toolset: McpToolsetView | null
  onClose: () => void
  onConfirm: () => void
}) {
  return (
    <AlertDialog
      open={Boolean(toolset)}
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Remove all tools?</AlertDialogTitle>
          <AlertDialogDescription>
            {toolset?.display_name} will have no tools. Existing access rules will stay in place.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={onConfirm}>
            Save empty tool set
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
