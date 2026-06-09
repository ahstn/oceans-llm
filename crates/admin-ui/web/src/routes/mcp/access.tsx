import { useState, useTransition, type FormEvent } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  addMcpToolset,
  disableExternalMcpToolset,
  getMcpEffectiveAccess,
  getMcpGrants,
  getMcpToolsets,
  removeMcpGrant,
  saveMcpGrant,
  saveMcpToolset,
  saveMcpToolsetTools,
} from '@/server/admin-data.functions'
import type { McpEffectiveAccessPayload, McpGrantView, McpToolsetView } from '@/types/api'

type ToolsetForm = {
  toolset_key: string
  display_name: string
  description: string
}

type GrantForm = {
  subject_kind: string
  subject_id: string
  target_kind: string
  target_id: string
}

type PreviewForm = {
  api_key_id: string
  user_id: string
  service_account_id: string
  team_id: string
  server_id: string
}

const subjectKinds = ['api_key', 'user', 'team', 'service_account'] as const
const targetKinds = ['tool', 'toolset'] as const

export const Route = createFileRoute('/mcp/access')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: async () => {
    const [toolsets, grants] = await Promise.all([
      getMcpToolsets({ data: { include_disabled: true } }),
      getMcpGrants(),
    ])
    return {
      toolsets: toolsets.data.items,
      grants: grants.data.items,
    }
  },
  component: McpAccessPage,
})

export function McpAccessPage() {
  const { toolsets, grants } = Route.useLoaderData()
  const router = useRouter()
  const [isPending, startTransition] = useTransition()
  const [toolsetForm, setToolsetForm] = useState<ToolsetForm>({
    toolset_key: '',
    display_name: '',
    description: '',
  })
  const [selectedToolsetId, setSelectedToolsetId] = useState(toolsets[0]?.id ?? '')
  const [toolIdsText, setToolIdsText] = useState('')
  const [grantForm, setGrantForm] = useState<GrantForm>({
    subject_kind: 'api_key',
    subject_id: '',
    target_kind: 'toolset',
    target_id: '',
  })
  const [previewForm, setPreviewForm] = useState<PreviewForm>({
    api_key_id: '',
    user_id: '',
    service_account_id: '',
    team_id: '',
    server_id: '',
  })
  const [preview, setPreview] = useState<McpEffectiveAccessPayload | null>(null)

  function refresh() {
    return router.invalidate()
  }

  function handleToolsetSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const input = {
      toolset_key: toolsetForm.toolset_key.trim(),
      display_name: toolsetForm.display_name.trim(),
      description: emptyToNull(toolsetForm.description),
    }
    if (!input.toolset_key || !input.display_name) {
      toast.error('Toolset key and display name are required')
      return
    }
    startTransition(async () => {
      try {
        const existing = toolsets.find((toolset) => toolset.toolset_key === input.toolset_key)
        if (existing) {
          await saveMcpToolset({ data: { toolsetId: existing.id, input } })
          toast.success('MCP toolset updated')
        } else {
          const created = await addMcpToolset({ data: input })
          setSelectedToolsetId(created.data.toolset.id)
          toast.success('MCP toolset created')
        }
        setToolsetForm({ toolset_key: '', display_name: '', description: '' })
        await refresh()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save MCP toolset')
      }
    })
  }

  function handleMembershipSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedToolsetId) {
      toast.error('Choose a toolset')
      return
    }
    const toolIds = toolIdsText
      .split(/[\s,]+/)
      .map((value) => value.trim())
      .filter(Boolean)
    startTransition(async () => {
      try {
        await saveMcpToolsetTools({ data: { toolsetId: selectedToolsetId, toolIds } })
        toast.success('Toolset membership replaced')
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to replace toolset membership')
      }
    })
  }

  function handleGrantSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!grantForm.subject_id.trim() || !grantForm.target_id.trim()) {
      toast.error('Subject ID and target ID are required')
      return
    }
    startTransition(async () => {
      try {
        await saveMcpGrant({ data: trimmedGrant(grantForm) })
        toast.success('MCP grant saved')
        await refresh()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save MCP grant')
      }
    })
  }

  function revokeGrant(grant: McpGrantView) {
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
        await refresh()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to revoke MCP grant')
      }
    })
  }

  function disableToolset(toolset: McpToolsetView) {
    startTransition(async () => {
      try {
        await disableExternalMcpToolset({ data: { toolsetId: toolset.id } })
        toast.success('MCP toolset disabled')
        await refresh()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to disable MCP toolset')
      }
    })
  }

  function handlePreviewSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const query = {
      api_key_id: emptyToUndefined(previewForm.api_key_id),
      user_id: emptyToUndefined(previewForm.user_id),
      service_account_id: emptyToUndefined(previewForm.service_account_id),
      team_id: emptyToUndefined(previewForm.team_id),
      server_id: emptyToUndefined(previewForm.server_id),
    }
    if (!query.api_key_id && !query.user_id && !query.service_account_id && !query.team_id) {
      toast.error('At least one subject ID is required')
      return
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
    <div className="flex min-w-0 flex-col gap-6">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-normal">MCP Access</h1>
          <p className="text-muted-foreground text-sm">
            Manage toolsets, explicit grants, and effective access previews.
          </p>
        </div>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(24rem,0.75fr)]">
        <Card>
          <CardHeader>
            <CardTitle>Toolsets</CardTitle>
            <CardDescription>Named bundles of active MCP tools.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            <form className="grid gap-3 md:grid-cols-3" onSubmit={handleToolsetSubmit}>
              <TextField
                id="toolset-key"
                label="Key"
                value={toolsetForm.toolset_key}
                onChange={(value) =>
                  setToolsetForm((current) => ({ ...current, toolset_key: value }))
                }
              />
              <TextField
                id="toolset-name"
                label="Display Name"
                value={toolsetForm.display_name}
                onChange={(value) =>
                  setToolsetForm((current) => ({ ...current, display_name: value }))
                }
              />
              <TextField
                id="toolset-description"
                label="Description"
                value={toolsetForm.description}
                onChange={(value) =>
                  setToolsetForm((current) => ({ ...current, description: value }))
                }
              />
              <div className="md:col-span-3">
                <Button type="submit" disabled={isPending}>
                  Save Toolset
                </Button>
              </div>
            </form>

            <div className="grid gap-2">
              {toolsets.map((toolset) => (
                <div
                  key={toolset.id}
                  className="border-border flex flex-wrap items-center justify-between gap-3 rounded-md border p-3"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{toolset.display_name}</span>
                      <Badge variant={toolset.status === 'active' ? 'default' : 'secondary'}>
                        {toolset.status}
                      </Badge>
                    </div>
                    <div className="text-muted-foreground truncate font-mono text-xs">
                      {toolset.toolset_key} · {toolset.id}
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={isPending || toolset.status !== 'active'}
                    onClick={() => disableToolset(toolset)}
                  >
                    Disable
                  </Button>
                </div>
              ))}
            </div>

            <form className="grid gap-3" onSubmit={handleMembershipSubmit}>
              <div className="grid gap-2">
                <FieldLabel htmlFor="membership-toolset">Toolset</FieldLabel>
                <Select value={selectedToolsetId} onValueChange={setSelectedToolsetId}>
                  <SelectTrigger id="membership-toolset">
                    <SelectValue placeholder="Choose toolset" />
                  </SelectTrigger>
                  <SelectContent>
                    {toolsets.map((toolset) => (
                      <SelectItem key={toolset.id} value={toolset.id}>
                        {toolset.display_name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-2">
                <FieldLabel htmlFor="membership-tool-ids">Tool IDs</FieldLabel>
                <Textarea
                  id="membership-tool-ids"
                  value={toolIdsText}
                  onChange={(event) => setToolIdsText(event.target.value)}
                  placeholder="UUIDs separated by commas or new lines"
                  className="min-h-24 font-mono text-xs"
                />
              </div>
              <Button type="submit" disabled={isPending}>
                Replace Membership
              </Button>
            </form>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Effective Preview</CardTitle>
            <CardDescription>Resolve callable tools for selected subjects.</CardDescription>
          </CardHeader>
          <CardContent>
            <form className="grid gap-3" onSubmit={handlePreviewSubmit}>
              {(
                ['api_key_id', 'user_id', 'service_account_id', 'team_id', 'server_id'] as const
              ).map((field) => (
                <TextField
                  key={field}
                  id={`preview-${field}`}
                  label={field.replaceAll('_', ' ')}
                  value={previewForm[field]}
                  onChange={(value) =>
                    setPreviewForm((current) => ({ ...current, [field]: value }))
                  }
                />
              ))}
              <Button type="submit" disabled={isPending}>
                Preview Access
              </Button>
            </form>

            {preview ? (
              <div className="mt-4 grid gap-3">
                <div className="grid grid-cols-3 gap-2 text-sm">
                  <Metric label="Servers" value={preview.referenced_server_count} />
                  <Metric label="Exposed" value={preview.exposed_tool_count} />
                  <Metric label="Filtered" value={preview.filtered_tool_count} />
                </div>
                <div className="grid gap-2">
                  {preview.tools.map((tool) => (
                    <div key={tool.id} className="border-border rounded-md border p-2">
                      <div className="font-medium">{tool.display_name}</div>
                      <div className="text-muted-foreground font-mono text-xs">
                        {tool.server_id} · {tool.upstream_name}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ) : null}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Grants</CardTitle>
          <CardDescription>Explicit active grants for tools and toolsets.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4">
          <form className="grid gap-3 md:grid-cols-5" onSubmit={handleGrantSubmit}>
            <SelectField
              id="grant-subject-kind"
              label="Subject"
              value={grantForm.subject_kind}
              values={subjectKinds}
              onChange={(value) => setGrantForm((current) => ({ ...current, subject_kind: value }))}
            />
            <TextField
              id="grant-subject-id"
              label="Subject ID"
              value={grantForm.subject_id}
              onChange={(value) => setGrantForm((current) => ({ ...current, subject_id: value }))}
            />
            <SelectField
              id="grant-target-kind"
              label="Target"
              value={grantForm.target_kind}
              values={targetKinds}
              onChange={(value) => setGrantForm((current) => ({ ...current, target_kind: value }))}
            />
            <TextField
              id="grant-target-id"
              label="Target ID"
              value={grantForm.target_id}
              onChange={(value) => setGrantForm((current) => ({ ...current, target_id: value }))}
            />
            <div className="flex items-end">
              <Button type="submit" disabled={isPending}>
                Save Grant
              </Button>
            </div>
          </form>

          <div className="grid gap-2">
            {grants.map((grant) => (
              <div
                key={grant.id}
                className="border-border grid gap-3 rounded-md border p-3 md:grid-cols-[1fr_1fr_auto]"
              >
                <GrantCell label={grant.subject_kind} value={grant.subject_id} />
                <GrantCell label={grant.target_kind} value={grant.target_id} />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={isPending || !grant.is_active}
                  onClick={() => revokeGrant(grant)}
                >
                  Revoke
                </Button>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

function TextField({
  id,
  label,
  value,
  onChange,
}: {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
}) {
  return (
    <div className="grid gap-2">
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input id={id} value={value} onChange={(event) => onChange(event.target.value)} />
    </div>
  )
}

function SelectField({
  id,
  label,
  value,
  values,
  onChange,
}: {
  id: string
  label: string
  value: string
  values: readonly string[]
  onChange: (value: string) => void
}) {
  return (
    <div className="grid gap-2">
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger id={id}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {values.map((item) => (
            <SelectItem key={item} value={item}>
              {item}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="bg-muted/40 rounded-md p-2">
      <div className="text-muted-foreground text-xs">{label}</div>
      <div className="font-mono text-lg">{value}</div>
    </div>
  )
}

function GrantCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-muted-foreground text-xs">{label}</div>
      <div className="truncate font-mono text-xs">{value}</div>
    </div>
  )
}

function emptyToNull(value: string) {
  const trimmed = value.trim()
  return trimmed ? trimmed : null
}

function emptyToUndefined(value: string) {
  const trimmed = value.trim()
  return trimmed ? trimmed : undefined
}

function trimmedGrant(form: GrantForm) {
  return {
    subject_kind: form.subject_kind,
    subject_id: form.subject_id.trim(),
    target_kind: form.target_kind,
    target_id: form.target_id.trim(),
  }
}
