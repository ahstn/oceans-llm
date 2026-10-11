import { useState } from 'react'
import { Cancel01Icon, LinkSquare02Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import {
  CodeBlock,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from '@/components/reui/code-block/code-block'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
  FieldTitle,
} from '@/components/ui/field'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from '@/components/ui/input-group'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { GuardrailPoliciesView } from '@/types/api'

import { REDACTION_TIERS, type PolicyDraft } from '../config-yaml'
import type { PolicyScope } from '../decision-meta'
import { groupPacks, kindLabel, MANAGED_KINDS, packInfo, scopeLabel, type AreaId } from './model'

type EditorProps = {
  draft: PolicyDraft
  policies: GuardrailPoliciesView
  onChange: (draft: PolicyDraft) => void
}

const toggle = (items: string[], item: string, on: boolean) =>
  on ? [...items, item] : items.filter((value) => value !== item)

export function AreaEditor({ area, ...props }: EditorProps & { area: AreaId }) {
  if (area === 'destructive') return <DestructiveActionsEditor {...props} />
  if (area === 'redaction') return <SecretRedactionEditor {...props} />
  return <IntegrationsEditor {...props} />
}

export function DestructiveActionsEditor({ draft, policies, onChange }: EditorProps) {
  return (
    <FieldGroup>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel htmlFor="destructive-enabled">Guardrails enabled</FieldLabel>
          <FieldDescription>
            Turning this off also stops redaction and managed checks.
          </FieldDescription>
        </FieldContent>
        <Switch
          id="destructive-enabled"
          checked={draft.enabled}
          onCheckedChange={(enabled) => onChange({ ...draft, enabled })}
        />
      </Field>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldTitle>Mode</FieldTitle>
          <FieldDescription>Audit records a match; deny stops the operation.</FieldDescription>
        </FieldContent>
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          aria-label="Mode"
          value={draft.mode}
          disabled={!draft.enabled}
          onValueChange={(mode) => mode && onChange({ ...draft, mode })}
        >
          <ToggleGroupItem value="audit">Audit</ToggleGroupItem>
          <ToggleGroupItem value="deny">Deny</ToggleGroupItem>
        </ToggleGroup>
      </Field>
      {groupPacks(policies.built_in_packs).map(([group, packs]) => (
        <FieldSet key={group}>
          <FieldLegend variant="label">{group}</FieldLegend>
          <FieldGroup className="gap-3">
            {packs.map((pack) => {
              const id = `pack-${pack.id.replace(/\W/g, '-')}`
              return (
                <Field key={pack.id} orientation="horizontal">
                  <FieldContent>
                    <FieldLabel htmlFor={id} className="font-mono text-xs">
                      {pack.id}
                      <Badge variant="outline">v{pack.version}</Badge>
                    </FieldLabel>
                    <FieldDescription>{packInfo(pack.id).description}</FieldDescription>
                  </FieldContent>
                  <Switch
                    id={id}
                    disabled={!draft.enabled}
                    checked={draft.packs.includes(pack.id)}
                    onCheckedChange={(on) =>
                      onChange({ ...draft, packs: toggle(draft.packs, pack.id, on) })
                    }
                  />
                </Field>
              )
            })}
          </FieldGroup>
        </FieldSet>
      ))}
    </FieldGroup>
  )
}

export function SecretRedactionEditor({ draft, onChange }: EditorProps) {
  const [rule, setRule] = useState('')
  const redaction = draft.redaction
  const set = (next: Partial<PolicyDraft['redaction']>) =>
    onChange({ ...draft, redaction: { ...redaction, ...next } })
  const addRule = () => {
    const value = rule.trim()
    if (value && !redaction.disabled_rules.includes(value))
      set({ disabled_rules: [...redaction.disabled_rules, value] })
    setRule('')
  }

  return (
    <FieldGroup>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel htmlFor="redaction-enabled">Redact secrets</FieldLabel>
          <FieldDescription>
            Applies to model-route prompts and request-log payloads.
          </FieldDescription>
        </FieldContent>
        <Switch
          id="redaction-enabled"
          checked={redaction.enabled}
          onCheckedChange={(enabled) => set({ enabled })}
        />
      </Field>
      <FieldSet>
        <FieldLegend variant="label">Tiers</FieldLegend>
        <FieldGroup data-slot="checkbox-group" className="gap-3">
          {REDACTION_TIERS.map((tier) => (
            <FieldLabel key={tier.id} htmlFor={`tier-${tier.id}`}>
              <Field orientation="horizontal">
                <Checkbox
                  id={`tier-${tier.id}`}
                  disabled={!redaction.enabled}
                  checked={redaction.tiers.includes(tier.id)}
                  onCheckedChange={(on) =>
                    set({ tiers: toggle(redaction.tiers, tier.id, on === true) })
                  }
                />
                <FieldContent>
                  <FieldTitle>
                    {tier.label}
                    {tier.id === 'generic' ? <Badge variant="warning">Noisy</Badge> : null}
                  </FieldTitle>
                  <FieldDescription>{tier.hint}</FieldDescription>
                </FieldContent>
              </Field>
            </FieldLabel>
          ))}
        </FieldGroup>
      </FieldSet>
      <Field>
        <FieldLabel htmlFor="redaction-disabled-rule">Disabled rules</FieldLabel>
        <FieldDescription>
          Rule IDs that cause false positives. Startup rejects unknown IDs.
        </FieldDescription>
        <InputGroup>
          <InputGroupInput
            id="redaction-disabled-rule"
            placeholder="jwt"
            value={rule}
            disabled={!redaction.enabled}
            onChange={(event) => setRule(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                addRule()
              }
            }}
          />
          <InputGroupAddon align="inline-end">
            <InputGroupButton size="xs" onClick={addRule}>
              Add
            </InputGroupButton>
          </InputGroupAddon>
        </InputGroup>
        {redaction.disabled_rules.length ? (
          <div className="flex flex-wrap gap-1">
            {redaction.disabled_rules.map((id) => (
              <Badge key={id} variant="outline" className="font-mono">
                {id}
                <button
                  type="button"
                  aria-label={`Re-enable ${id}`}
                  onClick={() =>
                    set({ disabled_rules: redaction.disabled_rules.filter((item) => item !== id) })
                  }
                >
                  <AppIcon icon={Cancel01Icon} size={12} aria-hidden />
                </button>
              </Badge>
            ))}
          </div>
        ) : null}
      </Field>
    </FieldGroup>
  )
}

export function IntegrationsEditor({ draft, policies, onChange }: EditorProps) {
  const configured = new Set(policies.managed_checks.map((check) => check.kind))
  return (
    <FieldGroup>
      <FieldSet>
        <FieldLegend variant="label">Configured checks</FieldLegend>
        {policies.managed_checks.length === 0 ? (
          <FieldDescription>
            No managed checks are defined under guardrails.managed_checks.
          </FieldDescription>
        ) : (
          <FieldGroup className="gap-3">
            {policies.managed_checks.map((check) => {
              const id = `check-${check.name.replace(/\W/g, '-')}`
              return (
                <Field key={check.name} orientation="horizontal">
                  <FieldContent>
                    <FieldLabel htmlFor={id}>
                      {check.name}
                      <Badge variant="outline">{kindLabel(check.kind)}</Badge>
                    </FieldLabel>
                    <FieldDescription>
                      {check.phases.length} phases, {check.timeout_ms} ms timeout, fails{' '}
                      {check.failure_disposition.replace('fail_', '')}
                    </FieldDescription>
                  </FieldContent>
                  <Switch
                    id={id}
                    disabled={!draft.enabled}
                    checked={draft.managed_checks.includes(check.name)}
                    onCheckedChange={(on) =>
                      onChange({
                        ...draft,
                        managed_checks: toggle(draft.managed_checks, check.name, on),
                      })
                    }
                  />
                </Field>
              )
            })}
          </FieldGroup>
        )}
      </FieldSet>
      <FieldSet>
        <FieldLegend variant="label">Available providers</FieldLegend>
        <FieldGroup className="gap-3">
          {MANAGED_KINDS.map((kind) => (
            <Field key={kind.kind} orientation="horizontal">
              <FieldContent>
                <FieldTitle>
                  {kind.label}
                  {configured.has(kind.kind) ? <Badge variant="success">Configured</Badge> : null}
                </FieldTitle>
                <FieldDescription>
                  Define the check in gateway.yaml, then enable it per scope.
                </FieldDescription>
              </FieldContent>
              <Button variant="outline" size="sm" asChild>
                <a href={kind.docs} target="_blank" rel="noreferrer">
                  Setup guide
                  <AppIcon icon={LinkSquare02Icon} data-icon="inline-end" aria-hidden />
                </a>
              </Button>
            </Field>
          ))}
        </FieldGroup>
      </FieldSet>
    </FieldGroup>
  )
}

export function ScopeSelect({
  scopes,
  value,
  onChange,
  className,
}: {
  scopes: PolicyScope[]
  value: string
  onChange: (key: string) => void
  className?: string
}) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label="Scope" className={className}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {scopes.map((scope) => (
            <SelectItem key={scope.key} value={scope.key}>
              {scopeLabel(scope)}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

export function YamlPreview({ yaml, maxLines }: { yaml: string; maxLines?: number }) {
  return (
    <CodeBlock code={yaml} language="yaml" maxLines={maxLines}>
      <CodeBlockHeader>
        <CodeBlockTitle>gateway.yaml</CodeBlockTitle>
        <CodeBlockCopyButton className="ml-auto" labels={{ copy: 'Copy YAML' }} />
      </CodeBlockHeader>
    </CodeBlock>
  )
}
