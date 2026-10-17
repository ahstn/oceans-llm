import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  getMcpTagRulePreview,
  removeMcpTagRule,
  saveMcpTagRule,
} from '@/server/mcp-policy-data.functions'
import type { McpServerView, McpTagRuleView, McpToolView } from '@/types/api'
import { derivedTags } from './-tool-policy'
import { EntityComboBox, type ComboOption } from './-tool-picker'

const ALL_SERVERS = 'all'

const effects = [
  { value: 'include', label: 'Include (grant matching tools)' },
  { value: 'exclude', label: 'Exclude (deny matching tools)' },
] as const

type RuleForm = {
  subject_kind: string
  subject_id: string
  server_id: string
  tag: string
  effect: string
}

/**
 * Rules that grant or deny tools by tag. Include rules cover tools discovered
 * later; exclude rules win over every grant the subject's callers hold.
 */
// Rule creation, preview, and revocation share the same form and option data.
// oxlint-disable-next-line eslint/max-lines-per-function
export function TagRulesSection({
  rules,
  servers,
  tools,
  subjectKinds,
  subjectOptions,
}: {
  rules: McpTagRuleView[]
  servers: McpServerView[]
  tools: McpToolView[]
  subjectKinds: readonly { value: string; label: string }[]
  subjectOptions: (kind: string) => ComboOption[]
}) {
  const router = useRouter()
  const [isPending, startTransition] = useTransition()
  const [form, setForm] = useState<RuleForm>({
    subject_kind: 'team',
    subject_id: '',
    server_id: '',
    tag: 'read_only',
    effect: 'include',
  })
  const [preview, setPreview] = useState<McpToolView[] | null>(null)

  const serverNameById = useMemo(
    () => new Map(servers.map((server) => [server.id, server.display_name])),
    [servers],
  )
  const serverOptions: ComboOption[] = [
    ...(form.effect === 'exclude' ? [{ value: ALL_SERVERS, label: 'All servers' }] : []),
    ...servers
      .filter((server) => server.status === 'active')
      .map((server) => ({
        value: server.id,
        label: server.display_name,
        sublabel: server.server_key,
      })),
  ]
  const tagOptions: ComboOption[] = useMemo(() => {
    const adminTags = new Set(tools.flatMap((tool) => tool.admin_tags))
    return [
      ...derivedTags.map((tag) => ({ value: tag, label: tag, sublabel: 'from annotations' })),
      ...[...adminTags].sort().map((tag) => ({ value: tag, label: tag, sublabel: 'admin tag' })),
    ]
  }, [tools])
  const activeRules = rules.filter((rule) => rule.is_active)

  function updateForm(next: Partial<RuleForm>) {
    setForm((current) => {
      const merged = { ...current, ...next }
      if (merged.effect === 'include' && merged.server_id === ALL_SERVERS) {
        merged.server_id = ''
      }
      return merged
    })
    setPreview(null)
  }

  function serverIdForRequest() {
    return form.server_id && form.server_id !== ALL_SERVERS ? form.server_id : undefined
  }

  function handlePreview() {
    if (!form.tag || (form.effect === 'include' && !serverIdForRequest())) {
      toast.error('Select a server and a tag to preview')
      return
    }
    startTransition(async () => {
      try {
        const response = await getMcpTagRulePreview({
          data: { server_id: serverIdForRequest(), tag: form.tag, effect: form.effect },
        })
        setPreview(response.data.tools)
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to preview the rule')
      }
    })
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!form.subject_id || !form.tag || !form.server_id) {
      toast.error('Select a subject, a server, and a tag')
      return
    }
    startTransition(async () => {
      try {
        await saveMcpTagRule({
          data: {
            subject_kind: form.subject_kind,
            subject_id: form.subject_id,
            server_id: serverIdForRequest(),
            tag: form.tag,
            effect: form.effect,
          },
        })
        toast.success('Tag rule saved')
        setPreview(null)
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save the tag rule')
      }
    })
  }

  function handleRevoke(rule: McpTagRuleView) {
    startTransition(async () => {
      try {
        await removeMcpTagRule({ data: { ruleId: rule.id } })
        toast.success('Tag rule revoked')
        await router.invalidate()
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to revoke the tag rule')
      }
    })
  }

  function subjectLabel(rule: McpTagRuleView) {
    return (
      subjectOptions(rule.subject_kind).find((option) => option.value === rule.subject_id)?.label ??
      rule.subject_id
    )
  }

  return (
    <section
      className="flex min-w-0 flex-col gap-4 rounded-md border p-4"
      data-testid="mcp-tag-rules"
    >
      <div className="flex flex-col gap-1">
        <h3 className="font-medium">Tag rules</h3>
        <p className="text-subtle-foreground text-sm">
          Grant or deny tools by behaviour or admin tag. Matching tools discovered later are covered
          automatically, and exclude rules win over every grant.
        </p>
      </div>

      <form className="flex flex-col gap-3" onSubmit={handleSubmit}>
        <FieldGroup className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          <Field>
            <FieldLabel>Subject type</FieldLabel>
            <PlainSelect
              ariaLabel="Tag rule subject type"
              value={form.subject_kind}
              options={subjectKinds}
              onChange={(value) => updateForm({ subject_kind: value, subject_id: '' })}
            />
          </Field>
          <Field>
            <FieldLabel>Subject</FieldLabel>
            <EntityComboBox
              ariaLabel="Tag rule subject"
              options={subjectOptions(form.subject_kind)}
              value={form.subject_id}
              onChange={(value) => updateForm({ subject_id: value })}
              placeholder="Select a subject"
              searchPlaceholder="Search subjects…"
            />
          </Field>
          <Field>
            <FieldLabel>Effect</FieldLabel>
            <PlainSelect
              ariaLabel="Tag rule effect"
              value={form.effect}
              options={effects}
              onChange={(value) => updateForm({ effect: value })}
            />
          </Field>
          <Field>
            <FieldLabel>Server</FieldLabel>
            <EntityComboBox
              ariaLabel="Tag rule server"
              options={serverOptions}
              value={form.server_id}
              onChange={(value) => updateForm({ server_id: value })}
              placeholder="Select a server"
              searchPlaceholder="Search servers…"
            />
            <FieldDescription>Only exclude rules can apply to all servers.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel>Tag</FieldLabel>
            <EntityComboBox
              ariaLabel="Tag rule tag"
              options={tagOptions}
              value={form.tag}
              onChange={(value) => updateForm({ tag: value })}
              placeholder="Select a tag"
              searchPlaceholder="Search tags…"
            />
          </Field>
        </FieldGroup>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={isPending} onClick={handlePreview}>
            Preview matches
          </Button>
          <Button type="submit" disabled={isPending}>
            Save rule
          </Button>
        </div>
      </form>

      {preview ? (
        <div className="flex flex-col gap-2" data-testid="mcp-tag-rule-preview">
          <div className="text-subtle-foreground text-xs font-medium">
            {preview.length} matching tool{preview.length === 1 ? '' : 's'}
          </div>
          {preview.map((tool) => (
            <div
              key={tool.id}
              className="flex min-w-0 items-center justify-between gap-2 border-t pt-2"
            >
              <div className="min-w-0">
                <div className="truncate font-medium">{tool.display_name}</div>
                <div className="text-subtle-foreground truncate font-mono text-xs">
                  {serverNameById.get(tool.server_id) ?? tool.server_id} · {tool.upstream_name}
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {activeRules.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No tag rules</EmptyTitle>
            <EmptyDescription>
              Grant a team every read-only tool on a server, or deny destructive tools.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="flex flex-col gap-2" data-testid="mcp-tag-rule-list">
          {activeRules.map((rule) => (
            <div
              key={rule.id}
              className="grid gap-3 rounded-md border p-3 md:grid-cols-[1fr_1fr_auto] md:items-center"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Badge variant="secondary">{rule.subject_kind}</Badge>
                  <span className="truncate font-medium">{subjectLabel(rule)}</span>
                </div>
              </div>
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <Badge variant={rule.effect === 'exclude' ? 'destructive' : 'success'}>
                  {rule.effect}
                </Badge>
                <span className="font-mono text-sm">{rule.tag}</span>
                <span className="text-subtle-foreground truncate text-sm">
                  on{' '}
                  {rule.server_id
                    ? (serverNameById.get(rule.server_id) ?? rule.server_id)
                    : 'all servers'}
                </span>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={isPending}
                onClick={() => handleRevoke(rule)}
              >
                Revoke
              </Button>
            </div>
          ))}
        </div>
      )}
    </section>
  )
}

function PlainSelect({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: string
  options: readonly { value: string; label: string }[]
  onChange: (value: string) => void
  ariaLabel: string
}) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label={ariaLabel}>
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
