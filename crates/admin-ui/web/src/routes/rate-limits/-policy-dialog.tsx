import { useState, useTransition, type FormEvent, type ReactNode } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { addRateLimitPolicy, saveRateLimitPolicy } from '@/server/admin-data.functions'
import type { RateLimitPolicyView } from '@/types/api'

import {
  ANY,
  OPERATION_LABELS,
  PERIOD_LABELS,
  SCOPE_LABELS,
  eachScopeLabel,
  emptyPolicyForm,
  formError,
  limitError,
  policyFormFromView,
  policyInputFromForm,
  type PolicyFormState,
} from './-policy-model'

export type PolicyOption = { value: string; label: string }

export type PolicyOptions = {
  apiKeys: PolicyOption[]
  users: PolicyOption[]
  teams: PolicyOption[]
  models: PolicyOption[]
  mcpServers: PolicyOption[]
}

type FieldsProps = {
  form: PolicyFormState
  update: (patch: Partial<PolicyFormState>) => void
  options: PolicyOptions
  pending: boolean
}

export function PolicyDialog({
  policy,
  options,
  onClose,
}: {
  policy: RateLimitPolicyView | null
  options: PolicyOptions
  onClose: () => void
}) {
  const router = useRouter()
  const [form, setForm] = useState<PolicyFormState>(() =>
    policy ? policyFormFromView(policy) : emptyPolicyForm(),
  )
  const [pending, startTransition] = useTransition()
  const error = formError(form)
  const fields: FieldsProps = {
    form,
    update: (patch) => setForm((current) => ({ ...current, ...patch })),
    options,
    pending,
  }

  function submit(event: FormEvent) {
    event.preventDefault()
    if (error) return
    const input = policyInputFromForm(form)
    startTransition(async () => {
      try {
        if (policy?.id) {
          await saveRateLimitPolicy({ data: { policyId: policy.id, input } })
        } else {
          await addRateLimitPolicy({ data: input })
        }
        await router.invalidate()
        toast.success(policy ? 'Rate limit saved' : 'Rate limit created')
        onClose()
      } catch (cause) {
        toast.error(cause instanceof Error ? cause.message : 'Failed to save rate limit')
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
          <DialogTitle>{policy ? 'Edit rate limit' : 'New rate limit'}</DialogTitle>
          <DialogDescription>
            Every matching policy applies. The most restrictive one rejects the request with 429.
          </DialogDescription>
        </DialogHeader>
        <form className="flex min-w-0 flex-col gap-4" onSubmit={submit}>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="rate-limit-name">Name</FieldLabel>
              <Input
                id="rate-limit-name"
                value={form.name}
                onChange={(event) => fields.update({ name: event.target.value })}
                required
                maxLength={100}
                disabled={pending}
              />
            </Field>
            <TrafficFields {...fields} />
            <ScopeFields {...fields} />
            <LimitFields {...fields} />
            <Field orientation="horizontal">
              <Switch
                id="rate-limit-enabled"
                checked={form.enabled}
                onCheckedChange={(enabled) => fields.update({ enabled })}
                disabled={pending}
              />
              <FieldLabel htmlFor="rate-limit-enabled">Enforced</FieldLabel>
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={pending}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending || Boolean(error)}>
              {pending ? 'Saving…' : policy ? 'Save rate limit' : 'Create rate limit'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** What the policy counts: the operation, narrowed to a model or an MCP server. */
function TrafficFields({ form, update, options, pending }: FieldsProps) {
  return (
    <div className="grid min-w-0 gap-4 sm:grid-cols-2">
      <PolicySelect
        id="rate-limit-operation"
        label="Traffic"
        value={form.operation}
        disabled={pending}
        onChange={(operation) =>
          update({
            operation: operation as PolicyFormState['operation'],
            metric: operation === 'inference' ? form.metric : 'requests',
          })
        }
      >
        {Object.entries(OPERATION_LABELS).map(([value, label]) => (
          <SelectItem key={value} value={value}>
            {label}
          </SelectItem>
        ))}
      </PolicySelect>
      {form.operation === 'mcp_tool_call' ? (
        <PolicySelect
          id="rate-limit-server"
          label="MCP server"
          value={form.mcpServerId}
          disabled={pending}
          onChange={(mcpServerId) => update({ mcpServerId })}
        >
          <SelectItem value={ANY}>Any server</SelectItem>
          <OptionItems options={options.mcpServers} />
        </PolicySelect>
      ) : (
        <PolicySelect
          id="rate-limit-model"
          label="Model"
          value={form.modelKey}
          disabled={pending}
          onChange={(modelKey) => update({ modelKey })}
        >
          <SelectItem value={ANY}>Any model</SelectItem>
          <OptionItems options={options.models} />
        </PolicySelect>
      )}
    </div>
  )
}

/** Who shares the counter: the whole gateway, each principal, or one principal. */
function ScopeFields({ form, update, options, pending }: FieldsProps) {
  const targets = { api_key: options.apiKeys, user: options.users, team: options.teams }
  return (
    <div className="grid min-w-0 gap-4 sm:grid-cols-2">
      <PolicySelect
        id="rate-limit-scope"
        label="Counted per"
        value={form.scopeKind}
        disabled={pending}
        onChange={(scopeKind) =>
          update({ scopeKind: scopeKind as PolicyFormState['scopeKind'], scopeId: ANY })
        }
      >
        {Object.entries(SCOPE_LABELS).map(([value, label]) => (
          <SelectItem key={value} value={value}>
            {label}
          </SelectItem>
        ))}
      </PolicySelect>
      {form.scopeKind !== 'global' && (
        <PolicySelect
          id="rate-limit-target"
          label="Applies to"
          value={form.scopeId}
          disabled={pending}
          onChange={(scopeId) => update({ scopeId })}
        >
          <SelectItem value={ANY}>{eachScopeLabel(form.scopeKind)}</SelectItem>
          <OptionItems options={targets[form.scopeKind]} />
        </PolicySelect>
      )}
    </div>
  )
}

function LimitFields({ form, update, pending }: FieldsProps) {
  const error = limitError(form)
  return (
    <>
      <div className="grid min-w-0 gap-4 sm:grid-cols-3">
        <Field data-invalid={Boolean(error) || undefined}>
          <FieldLabel htmlFor="rate-limit-limit">Limit</FieldLabel>
          <Input
            id="rate-limit-limit"
            type="number"
            inputMode="numeric"
            min={1}
            step={1}
            value={form.limit}
            onChange={(event) => update({ limit: event.target.value })}
            required
            disabled={pending}
            aria-invalid={Boolean(error) || undefined}
            aria-describedby={error ? 'rate-limit-limit-error' : undefined}
          />
          {error && <FieldError id="rate-limit-limit-error">{error}</FieldError>}
        </Field>
        <PolicySelect
          id="rate-limit-metric"
          label="Counting"
          value={form.operation === 'inference' ? form.metric : 'requests'}
          disabled={pending || form.operation !== 'inference'}
          onChange={(metric) => update({ metric: metric as PolicyFormState['metric'] })}
        >
          <SelectItem value="requests">Requests</SelectItem>
          <SelectItem value="tokens">Tokens</SelectItem>
        </PolicySelect>
        <PolicySelect
          id="rate-limit-period"
          label="Window"
          value={form.period}
          disabled={pending}
          onChange={(period) => update({ period: period as PolicyFormState['period'] })}
        >
          {Object.entries(PERIOD_LABELS).map(([value, label]) => (
            <SelectItem key={value} value={value}>
              {label}
            </SelectItem>
          ))}
        </PolicySelect>
      </div>
      <FieldDescription>
        Minute and hour limits slide. Day and month limits are quotas that reset at midnight UTC and
        on the first of the month. Token limits are charged after each response.
      </FieldDescription>
    </>
  )
}

function OptionItems({ options }: { options: PolicyOption[] }) {
  return options.map((option) => (
    <SelectItem key={option.value} value={option.value}>
      {option.label}
    </SelectItem>
  ))
}

function PolicySelect({
  id,
  label,
  value,
  disabled,
  onChange,
  children,
}: {
  id: string
  label: string
  value: string
  disabled?: boolean
  onChange: (value: string) => void
  children: ReactNode
}) {
  return (
    <Field className="min-w-0">
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Select value={value} onValueChange={onChange} disabled={disabled}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue placeholder={label} />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>{children}</SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}
