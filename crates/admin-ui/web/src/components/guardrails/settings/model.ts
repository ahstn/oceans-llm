import { useState } from 'react'
import { PlugSocketIcon, SquareLock02Icon, Delete02Icon } from '@hugeicons/core-free-icons'

import type { GuardrailPoliciesView } from '@/types/api'

import {
  configYaml,
  differingFields,
  draftChanged,
  draftFrom,
  overrideFields,
  REDACTION_TIERS,
  withFields,
  type PolicyDraft,
} from '../config-yaml'
import { policyScopes, type PolicyScope } from '../decision-meta'

export type AreaId = 'destructive' | 'redaction' | 'integrations'

/** The three things an operator configures; every mock gives each one its own surface. */
export const AREAS: { id: AreaId; label: string; description: string; icon: unknown }[] = [
  {
    id: 'destructive',
    label: 'Destructive actions',
    description: 'Packs that catch deletes, overwrites, and access removal.',
    icon: Delete02Icon,
  },
  {
    id: 'redaction',
    label: 'Secret redaction',
    description: 'Strip credentials from prompts before a provider sees them.',
    icon: SquareLock02Icon,
  },
  {
    id: 'integrations',
    label: 'Integrations',
    description: 'Managed checks from Amazon Bedrock and Google Model Armor.',
    icon: PlugSocketIcon,
  },
]

type PackInfo = { group: string; description: string }

/** Display metadata for built-in packs; ids the gateway adds later fall into "Other". */
const PACKS: Record<string, PackInfo> = {
  'core.shell': { group: 'Shell and files', description: 'Recursive deletes, forced overwrites' },
  'core.git': {
    group: 'Shell and files',
    description: 'Force pushes, history rewrites, branch deletes',
  },
  'core.filesystem': { group: 'Shell and files', description: 'Writes outside the workspace' },
  'database.postgresql': { group: 'Databases', description: 'DROP, TRUNCATE, unscoped DELETE' },
  'database.snowflake': { group: 'Databases', description: 'Dropping warehouses, schemas, tables' },
  'cloud.aws': { group: 'Cloud', description: 'Terminating, deleting, detaching resources' },
  'cloud.gcp': { group: 'Cloud', description: 'Deleting projects, instances, buckets' },
  'kubernetes.kubectl': { group: 'Kubernetes', description: 'Deleting namespaces and workloads' },
  'kubernetes.helm': { group: 'Kubernetes', description: 'Uninstalls and rollbacks' },
  'secrets.aws_secrets': {
    group: 'Secret managers',
    description: 'Deleting or overwriting secrets',
  },
  'secrets.onepassword': { group: 'Secret managers', description: 'Deleting vault items' },
  secret_disclosure: { group: 'Secret managers', description: 'Printing secret values to output' },
  'saas.github': { group: 'SaaS', description: 'Repo deletes, merges, workflow changes' },
  'saas.notion': { group: 'SaaS', description: 'Page and database deletion' },
}

export function packInfo(id: string): PackInfo {
  return PACKS[id] ?? { group: 'Other', description: 'Built-in pack' }
}

export function groupPacks(packs: GuardrailPoliciesView['built_in_packs']) {
  const groups = new Map<string, GuardrailPoliciesView['built_in_packs']>()
  for (const pack of packs) {
    const group = packInfo(pack.id).group
    groups.set(group, [...(groups.get(group) ?? []), pack])
  }
  return [...groups.entries()]
}

export const MANAGED_KINDS = [
  {
    kind: 'amazon_bedrock',
    label: 'Amazon Bedrock Guardrails',
    docs: 'https://github.com/ahstn/oceans-llm/blob/main/docs/operations/gateway-guardrails.md#amazon-bedrock-guardrails',
  },
  {
    kind: 'google_model_armor',
    label: 'Google Model Armor',
    docs: 'https://github.com/ahstn/oceans-llm/blob/main/docs/operations/gateway-guardrails.md#google-cloud-model-armor',
  },
]

export function kindLabel(kind: string) {
  return MANAGED_KINDS.find((item) => item.kind === kind)?.label ?? kind
}

const TIER_SHORT: Record<string, string> = {
  provider_tokens: 'Tokens',
  credentials: 'Credentials',
  generic: 'Generic',
}

/** One-line state of an area for a scope, for rows, cards and nav badges. */
export function areaSummary(area: AreaId, draft: PolicyDraft) {
  if (area === 'destructive') {
    if (!draft.enabled) return { text: 'Off', tone: 'outline' as const }
    return {
      text: `${draft.packs.length} ${draft.packs.length === 1 ? 'pack' : 'packs'}, ${draft.mode}`,
      tone: draft.mode === 'deny' ? ('success' as const) : ('warning' as const),
    }
  }
  if (area === 'redaction') {
    if (!draft.redaction.enabled || !draft.enabled) return { text: 'Off', tone: 'outline' as const }
    const labels = REDACTION_TIERS.filter((tier) => draft.redaction.tiers.includes(tier.id))
    return {
      text: labels.map((tier) => TIER_SHORT[tier.id]).join(', ') || 'No tiers',
      tone: 'success' as const,
    }
  }
  if (!draft.enabled) return { text: 'Off', tone: 'outline' as const }
  if (!draft.managed_checks.length) return { text: 'None', tone: 'outline' as const }
  return { text: draft.managed_checks.join(', '), tone: 'success' as const }
}

type Override = { values: PolicyDraft; explicit: Set<string> }
type DraftState = { base: PolicyDraft; overrides: Record<string, Override> }

function effective(state: DraftState, key: string) {
  const override = state.overrides[key]
  return override ? withFields(state.base, override.values, override.explicit) : state.base
}

/**
 * Draft state for every scope at once, so a mock can show one YAML document for all edits.
 * Overrides keep the fields they set themselves; everything else follows the default as it is
 * edited, which is how the gateway resolves an override.
 */
export function useGuardrailDrafts(policies: GuardrailPoliciesView) {
  const scopes = policyScopes(policies)
  const initial = (): DraftState => {
    const base = draftFrom(policies.default)
    const overrides: Record<string, Override> = {}
    for (const scope of scopes.filter((item) => item.kind !== 'global')) {
      const values = draftFrom(scope.policy)
      const explicit = new Set([...overrideFields(scope.policy), ...differingFields(values, base)])
      overrides[scope.key] = { values, explicit }
    }
    return { base, overrides }
  }
  const [state, setState] = useState(initial)
  const drafts = Object.fromEntries(scopes.map((scope) => [scope.key, effective(state, scope.key)]))
  const changed = scopes.filter((scope) => draftChanged(drafts[scope.key], draftFrom(scope.policy)))

  const update = (key: string, next: PolicyDraft) =>
    setState((current) => {
      const override = current.overrides[key]
      if (!override) return { ...current, base: next }
      const edited = differingFields(next, effective(current, key))
      const explicit = new Set([...override.explicit, ...edited])
      return {
        ...current,
        overrides: {
          ...current.overrides,
          [key]: { values: withFields(override.values, next, edited), explicit },
        },
      }
    })

  return {
    scopes,
    drafts,
    changed,
    yaml: configYaml(scopes, drafts, (scope) => state.overrides[scope.key]?.explicit ?? new Set()),
    update,
    reset: () => setState(initial()),
  }
}

export type GuardrailDrafts = ReturnType<typeof useGuardrailDrafts>

export function scopeLabel(scope: PolicyScope) {
  if (scope.kind === 'global') return 'Global default'
  return scope.name
}

export function scopeKindLabel(scope: PolicyScope) {
  return { global: 'Applies everywhere', model: 'Model route', mcp: 'MCP server' }[scope.kind]
}
