import { useState } from 'react'

import {
  CodeBlock,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from '@/components/reui/code-block/code-block'
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
} from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { GuardrailPoliciesView } from '@/types/api'

import { REDACTION_TIERS, draftFrom, scopeYaml, type PolicyDraft } from './config-yaml'
import { policyScopes, type PolicyScope } from './decision-meta'

/**
 * Drafts guardrail and secret-redaction settings as `gateway.yaml`. Configuration stays the only
 * policy authority, so nothing is saved from here: operators copy the YAML and redeploy.
 */
export function GuardrailConfigSheet({
  policies,
  open,
  onOpenChange,
}: {
  policies: GuardrailPoliciesView
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const scopes = policyScopes(policies)
  const [scopeKey, setScopeKey] = useState(scopes[0].key)
  const scope = scopes.find((item) => item.key === scopeKey) ?? scopes[0]

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="flex flex-col sm:max-w-lg">
        <SheetHeader>
          <SheetTitle>Configure guardrails</SheetTitle>
          <SheetDescription>
            Draft changes for one scope. Nothing is saved here; merge the excerpt into gateway.yaml
            and redeploy. Other scopes and managed check definitions stay as they are.
          </SheetDescription>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-4">
          <Field>
            <FieldLabel htmlFor="guardrail-config-scope">Scope</FieldLabel>
            <Select value={scope.key} onValueChange={setScopeKey}>
              <SelectTrigger id="guardrail-config-scope" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {scopes.map((item) => (
                    <SelectItem key={item.key} value={item.key}>
                      {scopeLabel(item)}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          <ScopeDraft key={scope.key} scope={scope} policies={policies} />
        </div>
      </SheetContent>
    </Sheet>
  )
}

const toggle = (items: string[], item: string, on: boolean) =>
  on ? [...items, item] : items.filter((value) => value !== item)

function ScopeDraft({ scope, policies }: { scope: PolicyScope; policies: GuardrailPoliciesView }) {
  const [draft, setDraft] = useState<PolicyDraft>(() => draftFrom(scope.policy))
  const yaml = scopeYaml(scope, draft, draftFrom(policies.default))
  return (
    <>
      <FieldSet>
        <FieldLegend>Guardrail policy</FieldLegend>
        <FieldGroup>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel htmlFor="guardrail-config-enabled">Enabled</FieldLabel>
              <FieldDescription>Run packs and managed checks in this scope.</FieldDescription>
            </FieldContent>
            <Switch
              id="guardrail-config-enabled"
              checked={draft.enabled}
              onCheckedChange={(enabled) => setDraft({ ...draft, enabled })}
            />
          </Field>
          <Field>
            <FieldLabel>Mode</FieldLabel>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              aria-label="Mode"
              value={draft.mode}
              onValueChange={(mode) => mode && setDraft({ ...draft, mode })}
            >
              <ToggleGroupItem value="audit">Audit</ToggleGroupItem>
              <ToggleGroupItem value="deny">Deny</ToggleGroupItem>
            </ToggleGroup>
            <FieldDescription>Audit records matches; deny stops the operation.</FieldDescription>
          </Field>
          <CheckList
            legend="Deterministic packs"
            options={policies.built_in_packs.map((pack) => ({
              id: pack.id,
              label: pack.id,
              hint: `v${pack.version}`,
            }))}
            selected={draft.packs}
            onChange={(id, on) => setDraft({ ...draft, packs: toggle(draft.packs, id, on) })}
          />
          <CheckList
            legend="Managed checks"
            empty="No managed checks are defined under guardrails.managed_checks."
            options={policies.managed_checks.map((check) => ({
              id: check.name,
              label: check.name,
              hint: `${check.kind}, fails ${check.failure_disposition}`,
            }))}
            selected={draft.managed_checks}
            onChange={(id, on) =>
              setDraft({ ...draft, managed_checks: toggle(draft.managed_checks, id, on) })
            }
          />
        </FieldGroup>
      </FieldSet>

      <FieldSet>
        <FieldLegend>Secret redaction</FieldLegend>
        <FieldGroup>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel htmlFor="guardrail-config-redaction">Enabled</FieldLabel>
              <FieldDescription>
                {scope.kind === 'mcp'
                  ? 'Redacts captured request-log payloads only; MCP arguments are forwarded unchanged.'
                  : 'Replace secrets in prompts before they reach a provider.'}
              </FieldDescription>
            </FieldContent>
            <Switch
              id="guardrail-config-redaction"
              checked={draft.redaction.enabled}
              onCheckedChange={(enabled) =>
                setDraft({ ...draft, redaction: { ...draft.redaction, enabled } })
              }
            />
          </Field>
          <CheckList
            legend="Tiers"
            mono={false}
            options={REDACTION_TIERS.map((tier) => ({ ...tier }))}
            selected={draft.redaction.tiers}
            onChange={(id, on) =>
              setDraft({
                ...draft,
                redaction: { ...draft.redaction, tiers: toggle(draft.redaction.tiers, id, on) },
              })
            }
          />
        </FieldGroup>
      </FieldSet>

      <CodeBlock code={yaml} language="yaml">
        <CodeBlockHeader>
          <CodeBlockTitle>gateway.yaml excerpt</CodeBlockTitle>
          <CodeBlockCopyButton className="ml-auto" labels={{ copy: 'Copy YAML' }} />
        </CodeBlockHeader>
      </CodeBlock>
      <SheetFooter className="px-0">
        <Button variant="outline" onClick={() => setDraft(draftFrom(scope.policy))}>
          Reset
        </Button>
      </SheetFooter>
    </>
  )
}

function CheckList({
  legend,
  options,
  selected,
  onChange,
  empty,
  mono = true,
}: {
  mono?: boolean
  legend: string
  options: { id: string; label: string; hint: string }[]
  selected: string[]
  onChange: (id: string, on: boolean) => void
  empty?: string
}) {
  const chosen = new Set(selected)
  return (
    <FieldSet>
      <FieldLegend variant="label">{legend}</FieldLegend>
      {options.length === 0 ? (
        <FieldDescription>{empty}</FieldDescription>
      ) : (
        <FieldGroup data-slot="checkbox-group" className="grid gap-3 sm:grid-cols-2">
          {options.map((option) => {
            const id = `guardrail-config-${legend}-${option.id}`.replace(/[^\w-]/g, '_')
            return (
              <Field key={option.id} orientation="horizontal">
                <Checkbox
                  id={id}
                  checked={chosen.has(option.id)}
                  onCheckedChange={(checked) => onChange(option.id, checked === true)}
                />
                <FieldContent>
                  <FieldLabel htmlFor={id} className={mono ? 'font-mono text-xs' : undefined}>
                    {option.label}
                  </FieldLabel>
                  <FieldDescription className="text-xs">{option.hint}</FieldDescription>
                </FieldContent>
              </Field>
            )
          })}
        </FieldGroup>
      )}
    </FieldSet>
  )
}

function scopeLabel(scope: PolicyScope) {
  if (scope.kind === 'global') return 'Global default'
  return `${scope.kind === 'model' ? 'Model route' : 'MCP server'}: ${scope.name}`
}
