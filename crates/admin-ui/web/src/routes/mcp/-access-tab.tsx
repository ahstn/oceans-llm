import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { getMcpEffectiveAccess, removeMcpGrant, saveMcpGrant } from '@/server/admin-data.functions'
import type {
  ApiKeyOwnerServiceAccountView,
  ApiKeyOwnerUserView,
  ApiKeyView,
  McpEffectiveAccessPayload,
  McpGrantView,
  McpServerView,
  McpToolsetView,
  AdminTeamOption,
} from '@/types/api'
import { useToolCatalog } from './-catalog'
import { EntityComboBox, type ComboOption } from './-tool-picker'

const subjectKinds = [
  { value: 'user', label: 'User' },
  { value: 'team', label: 'Team' },
  { value: 'api_key', label: 'API key' },
  { value: 'service_account', label: 'Service account' },
] as const

const targetKinds = [
  { value: 'toolset', label: 'Toolset' },
  { value: 'tool', label: 'Tool' },
] as const

const subjectQueryField: Record<
  string,
  'api_key_id' | 'user_id' | 'team_id' | 'service_account_id'
> = {
  api_key: 'api_key_id',
  user: 'user_id',
  team: 'team_id',
  service_account: 'service_account_id',
}

export type AccessSubjects = {
  apiKeys: ApiKeyView[]
  users: ApiKeyOwnerUserView[]
  serviceAccounts: ApiKeyOwnerServiceAccountView[]
  teams: AdminTeamOption[]
}

export function AccessTab({
  grants,
  servers,
  toolsets,
  subjects,
}: {
  grants: McpGrantView[]
  servers: McpServerView[]
  toolsets: McpToolsetView[]
  subjects: AccessSubjects
}) {
  const router = useRouter()
  const catalog = useToolCatalog(servers)
  const [isPending, startTransition] = useTransition()
  const [grantForm, setGrantForm] = useState({
    subject_kind: 'user',
    subject_id: '',
    target_kind: 'toolset',
    target_id: '',
  })
  const [previewForm, setPreviewForm] = useState({
    subject_kind: 'user',
    subject_id: '',
    server_id: '',
  })
  const [preview, setPreview] = useState<McpEffectiveAccessPayload | null>(null)

  const serverNameById = useMemo(
    () => new Map(servers.map((server) => [server.id, server.display_name])),
    [servers],
  )

  const subjectOptions = (kind: string): ComboOption[] => {
    switch (kind) {
      case 'api_key':
        return subjects.apiKeys.map((key) => ({
          value: key.id,
          label: key.name,
          sublabel: key.prefix,
        }))
      case 'team':
        return subjects.teams.map((team) => ({ value: team.id, label: team.name }))
      case 'service_account':
        return subjects.serviceAccounts.map((account) => ({
          value: account.id,
          label: account.name,
          sublabel: account.key,
        }))
      default:
        return subjects.users.map((user) => ({
          value: user.id,
          label: user.name,
          sublabel: user.email,
        }))
    }
  }

  const toolOptions: ComboOption[] = catalog.tools.map((tool) => ({
    value: tool.id,
    label: tool.display_name,
    sublabel: `${serverNameById.get(tool.server_id) ?? 'server'} · ${tool.upstream_name}`,
  }))

  const toolsetOptions: ComboOption[] = toolsets
    .filter((toolset) => toolset.status === 'active')
    .map((toolset) => ({
      value: toolset.id,
      label: toolset.display_name,
      sublabel: toolset.toolset_key,
    }))

  const serverOptions: ComboOption[] = servers.map((server) => ({
    value: server.id,
    label: server.display_name,
    sublabel: server.server_key,
  }))

  function resolveSubjectLabel(grant: McpGrantView) {
    const option = subjectOptions(grant.subject_kind).find(
      (entry) => entry.value === grant.subject_id,
    )
    return option?.label ?? grant.subject_id
  }

  function resolveTargetLabel(grant: McpGrantView) {
    if (grant.target_kind === 'toolset') {
      return (
        toolsets.find((toolset) => toolset.id === grant.target_id)?.display_name ?? grant.target_id
      )
    }
    return catalog.byId.get(grant.target_id)?.display_name ?? grant.target_id
  }

  function handleGrantSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!grantForm.subject_id || !grantForm.target_id) {
      toast.error('Select a subject and a target')
      return
    }
    startTransition(async () => {
      try {
        await saveMcpGrant({ data: grantForm })
        toast.success('MCP grant saved')
        setGrantForm((current) => ({ ...current, subject_id: '', target_id: '' }))
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save MCP grant')
      }
    })
  }

  function handleRevokeGrant(grant: McpGrantView) {
    startTransition(async () => {
      try {
        await removeMcpGrant({
          data: {
            subject_kind: grant.subject_kind,
            subject_id: grant.subject_id,
            target_kind: grant.target_kind,
            target_id: grant.target_id,
          },
        })
        toast.success('MCP grant revoked')
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to revoke MCP grant')
      }
    })
  }

  function handlePreviewSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!previewForm.subject_id) {
      toast.error('Select a subject to preview')
      return
    }
    const query: Record<string, string> = {
      [subjectQueryField[previewForm.subject_kind]]: previewForm.subject_id,
    }
    if (previewForm.server_id) {
      query.server_id = previewForm.server_id
    }
    startTransition(async () => {
      try {
        const response = await getMcpEffectiveAccess({ data: query })
        setPreview(response.data)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to preview MCP access')
      }
    })
  }

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="grid min-w-0 gap-4 xl:grid-cols-2">
        <section className="flex min-w-0 flex-col gap-4 rounded-md border p-4">
          <div className="flex flex-col gap-1">
            <h3 className="font-medium">New grant</h3>
            <p className="text-sm text-[var(--color-text-muted)]">
              Assign a tool or toolset to a subject — pick from real entities, no UUIDs.
            </p>
          </div>
          <form className="flex flex-col gap-3" onSubmit={handleGrantSubmit}>
            <FieldGroup className="grid gap-3 md:grid-cols-2">
              <Field>
                <FieldLabel>Subject type</FieldLabel>
                <KindSelect
                  value={grantForm.subject_kind}
                  options={subjectKinds}
                  onChange={(value) =>
                    setGrantForm((current) => ({ ...current, subject_kind: value, subject_id: '' }))
                  }
                />
              </Field>
              <Field>
                <FieldLabel>Subject</FieldLabel>
                <EntityComboBox
                  ariaLabel="Grant subject"
                  options={subjectOptions(grantForm.subject_kind)}
                  value={grantForm.subject_id}
                  onChange={(value) =>
                    setGrantForm((current) => ({ ...current, subject_id: value }))
                  }
                  placeholder="Select a subject"
                  searchPlaceholder="Search subjects…"
                />
              </Field>
              <Field>
                <FieldLabel>Target type</FieldLabel>
                <KindSelect
                  value={grantForm.target_kind}
                  options={targetKinds}
                  onChange={(value) =>
                    setGrantForm((current) => ({ ...current, target_kind: value, target_id: '' }))
                  }
                />
              </Field>
              <Field>
                <FieldLabel>Target</FieldLabel>
                <EntityComboBox
                  ariaLabel="Grant target"
                  options={grantForm.target_kind === 'tool' ? toolOptions : toolsetOptions}
                  value={grantForm.target_id}
                  onChange={(value) =>
                    setGrantForm((current) => ({ ...current, target_id: value }))
                  }
                  placeholder={
                    grantForm.target_kind === 'tool' ? 'Select a tool' : 'Select a toolset'
                  }
                  searchPlaceholder="Search targets…"
                />
              </Field>
            </FieldGroup>
            <div className="flex justify-end">
              <Button type="submit" disabled={isPending}>
                Save grant
              </Button>
            </div>
          </form>
        </section>

        <section className="flex min-w-0 flex-col gap-4 rounded-md border p-4">
          <div className="flex flex-col gap-1">
            <h3 className="font-medium">Effective access</h3>
            <p className="text-sm text-[var(--color-text-muted)]">
              Resolve the callable tools a subject actually sees.
            </p>
          </div>
          <form className="flex flex-col gap-3" onSubmit={handlePreviewSubmit}>
            <FieldGroup className="grid gap-3 md:grid-cols-2">
              <Field>
                <FieldLabel>Subject type</FieldLabel>
                <KindSelect
                  value={previewForm.subject_kind}
                  options={subjectKinds}
                  onChange={(value) =>
                    setPreviewForm((current) => ({
                      ...current,
                      subject_kind: value,
                      subject_id: '',
                    }))
                  }
                />
              </Field>
              <Field>
                <FieldLabel>Subject</FieldLabel>
                <EntityComboBox
                  ariaLabel="Preview subject"
                  options={subjectOptions(previewForm.subject_kind)}
                  value={previewForm.subject_id}
                  onChange={(value) =>
                    setPreviewForm((current) => ({ ...current, subject_id: value }))
                  }
                  placeholder="Select a subject"
                  searchPlaceholder="Search subjects…"
                />
              </Field>
              <Field className="md:col-span-2">
                <FieldLabel>Server (optional)</FieldLabel>
                <EntityComboBox
                  ariaLabel="Preview server"
                  options={serverOptions}
                  value={previewForm.server_id}
                  onChange={(value) =>
                    setPreviewForm((current) => ({ ...current, server_id: value }))
                  }
                  placeholder="All servers"
                  searchPlaceholder="Search servers…"
                />
              </Field>
            </FieldGroup>
            <div className="flex justify-end">
              <Button type="submit" disabled={isPending}>
                Preview access
              </Button>
            </div>
          </form>

          {preview ? <EffectivePreview preview={preview} serverNameById={serverNameById} /> : null}
        </section>
      </div>

      <section className="flex min-w-0 flex-col gap-3 rounded-md border p-4">
        <div className="flex flex-col gap-1">
          <h3 className="font-medium">Grants</h3>
          <p className="text-sm text-[var(--color-text-muted)]">
            Explicit active grants for tools and toolsets.
          </p>
        </div>
        {grants.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No grants</EmptyTitle>
              <EmptyDescription>Assign a toolset to a subject to get started.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="flex flex-col gap-2" data-testid="mcp-grant-list">
            {grants.map((grant) => (
              <div
                key={grant.id}
                className="grid gap-3 rounded-md border p-3 md:grid-cols-[1fr_1fr_auto] md:items-center"
              >
                <GrantCell
                  kind={grant.subject_kind}
                  label={resolveSubjectLabel(grant)}
                  id={grant.subject_id}
                />
                <GrantCell
                  kind={grant.target_kind}
                  label={resolveTargetLabel(grant)}
                  id={grant.target_id}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={isPending || !grant.is_active}
                  onClick={() => handleRevokeGrant(grant)}
                >
                  Revoke
                </Button>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

function KindSelect({
  value,
  options,
  onChange,
}: {
  value: string
  options: readonly { value: string; label: string }[]
  onChange: (value: string) => void
}) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

function EffectivePreview({
  preview,
  serverNameById,
}: {
  preview: McpEffectiveAccessPayload
  serverNameById: Map<string, string>
}) {
  const groups = useMemo(() => {
    const map = new Map<string, typeof preview.tools>()
    for (const tool of preview.tools) {
      const existing = map.get(tool.server_id)
      if (existing) {
        existing.push(tool)
      } else {
        map.set(tool.server_id, [tool])
      }
    }
    return [...map.entries()]
  }, [preview])

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-3 gap-2">
        <Metric label="Servers" value={preview.referenced_server_count} />
        <Metric label="Exposed" value={preview.exposed_tool_count} />
        <Metric label="Filtered" value={preview.filtered_tool_count} />
      </div>
      {preview.tools.length === 0 ? (
        <p className="text-sm text-[var(--color-text-muted)]">
          No callable tools for this subject.
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          {groups.map(([serverId, tools]) => (
            <div key={serverId} className="flex flex-col gap-1">
              <div className="text-xs font-medium text-[var(--color-text-soft)]">
                {serverNameById.get(serverId) ?? serverId}
              </div>
              <div className="grid gap-2">
                {tools.map((tool) => (
                  <div key={tool.id} className="rounded-md border p-2">
                    <div className="font-medium">{tool.display_name}</div>
                    <div className="font-mono text-xs text-[var(--color-text-muted)]">
                      {tool.upstream_name}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md bg-[var(--color-muted)] p-2">
      <div className="text-xs text-[var(--color-text-muted)]">{label}</div>
      <div className="font-mono text-lg">{value}</div>
    </div>
  )
}

function GrantCell({ kind, label, id }: { kind: string; label: string; id: string }) {
  return (
    <div className="min-w-0">
      <div className="flex items-center gap-2">
        <Badge variant="secondary">{kind}</Badge>
        <span className="truncate font-medium">{label}</span>
      </div>
      <div className="truncate font-mono text-xs text-[var(--color-text-muted)]">{id}</div>
    </div>
  )
}
