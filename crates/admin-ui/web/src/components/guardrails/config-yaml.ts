import type { GuardrailPolicy, PolicyScope } from './decision-meta'

export const REDACTION_TIERS = [
  {
    id: 'provider_tokens',
    label: 'Provider tokens',
    hint: 'Prefixed keys such as sk-, ghp_, AKIA',
  },
  { id: 'credentials', label: 'Credentials', hint: 'Passwords, bearer tokens, URI passwords' },
  { id: 'generic', label: 'Generic', hint: 'High-entropy values; more false positives' },
] as const

export type PolicyDraft = {
  enabled: boolean
  mode: string
  packs: string[]
  managed_checks: string[]
  redaction: { enabled: boolean; tiers: string[]; disabled_rules: string[] }
}

export function draftFrom(policy: GuardrailPolicy): PolicyDraft {
  return {
    enabled: policy.enabled,
    mode: policy.mode,
    packs: [...policy.packs],
    managed_checks: [...policy.managed_checks],
    redaction: {
      enabled: policy.secret_redaction.enabled,
      tiers: [...policy.secret_redaction.tiers],
      disabled_rules: [...policy.secret_redaction.disabled_rules],
    },
  }
}

type FieldId = (typeof FIELDS)[number]['id']

/** Every setting an override can set on its own; unset ones inherit the default's value. */
export const FIELDS = [
  { id: 'enabled', get: (d: PolicyDraft) => d.enabled },
  { id: 'mode', get: (d: PolicyDraft) => d.mode },
  { id: 'packs', get: (d: PolicyDraft) => d.packs },
  { id: 'managed_checks', get: (d: PolicyDraft) => d.managed_checks },
  { id: 'redaction.enabled', get: (d: PolicyDraft) => d.redaction.enabled },
  { id: 'redaction.tiers', get: (d: PolicyDraft) => d.redaction.tiers },
  { id: 'redaction.disabled_rules', get: (d: PolicyDraft) => d.redaction.disabled_rules },
] as const

/** Fields whose values differ between two drafts. */
export function differingFields(a: PolicyDraft, b: PolicyDraft): Set<FieldId> {
  const fields = new Set<FieldId>()
  for (const field of FIELDS) if (!same(field.get(a), field.get(b))) fields.add(field.id)
  return fields
}

/** Copies `fields` from `source` onto `target`, leaving the rest of `target` alone. */
export function withFields(target: PolicyDraft, source: PolicyDraft, fields: Set<string>) {
  const pick = <T>(id: string, own: T, other: T) => (fields.has(id) ? other : own)
  return {
    enabled: pick('enabled', target.enabled, source.enabled),
    mode: pick('mode', target.mode, source.mode),
    packs: pick('packs', target.packs, source.packs),
    managed_checks: pick('managed_checks', target.managed_checks, source.managed_checks),
    redaction: {
      enabled: pick('redaction.enabled', target.redaction.enabled, source.redaction.enabled),
      tiers: pick('redaction.tiers', target.redaction.tiers, source.redaction.tiers),
      disabled_rules: pick(
        'redaction.disabled_rules',
        target.redaction.disabled_rules,
        source.redaction.disabled_rules,
      ),
    },
  }
}

function policyLines(draft: PolicyDraft, fields: Set<string>, bufferBytes?: number) {
  const lines: string[] = []
  const value = (v: boolean | string | string[]) => (Array.isArray(v) ? list(v) : String(v))
  for (const field of FIELDS.slice(0, 4))
    if (fields.has(field.id)) lines.push(`${field.id}: ${value(field.get(draft))}`)
  if (bufferBytes !== undefined) lines.push(`stream_buffer_bytes: ${bufferBytes}`)
  const redaction: string[] = []
  for (const field of FIELDS.slice(4))
    if (fields.has(field.id))
      redaction.push(`  ${field.id.replace('redaction.', '')}: ${value(field.get(draft))}`)
  if (redaction.length) lines.push('secret_redaction:', ...redaction)
  return lines
}

/** True when an override's draft differs from the default it inherits. */
export function draftChanged(draft: PolicyDraft, base: PolicyDraft) {
  return differingFields(draft, base).size > 0
}

const ALL_FIELDS = new Set<string>(FIELDS.map((field) => field.id))

/**
 * Renders the `guardrails:` YAML for one scope. The default block is written in full; an override
 * lists only what differs from the default, because the gateway inherits every unset field.
 */
export function scopeYaml(scope: PolicyScope, draft: PolicyDraft, base: PolicyDraft) {
  return configYaml([scope], { [scope.key]: draft }, (item) =>
    item.kind === 'global' ? ALL_FIELDS : differingFields(draft, base),
  )
}

/**
 * Renders one `guardrails:` document for every scope in `drafts`, grouped by section.
 * `explicit` names the fields each override sets itself; the default is always written in full.
 */
export function configYaml(
  scopes: PolicyScope[],
  drafts: Record<string, PolicyDraft>,
  explicit: (scope: PolicyScope) => Set<string>,
) {
  const out = ['guardrails:']
  const sections = { model: 'model_routes', mcp: 'mcp_servers' } as const
  for (const scope of scopes.filter((item) => item.kind === 'global' && drafts[item.key])) {
    const lines = policyLines(drafts[scope.key], ALL_FIELDS, scope.policy.stream_buffer_bytes)
    out.push('  default:', ...lines.map((line) => `    ${line}`))
  }
  for (const kind of ['model', 'mcp'] as const) {
    const entries = scopes.filter((item) => item.kind === kind && drafts[item.key])
    if (!entries.length) continue
    out.push(`  ${sections[kind]}:`)
    for (const scope of entries) {
      const lines = policyLines(drafts[scope.key], explicit(scope))
      if (!lines.length) out.push(`    ${key(scope.name)}: {}`)
      else out.push(`    ${key(scope.name)}:`, ...lines.map((line) => `      ${line}`))
    }
  }
  return `${out.join('\n')}\n`
}

function same(a: boolean | string | readonly string[], b: boolean | string | readonly string[]) {
  return Array.isArray(a) && Array.isArray(b) ? sameSet(a, b) : a === b
}

function sameSet(a: readonly string[], b: readonly string[]) {
  const items = new Set(b)
  return a.length === b.length && a.every((item) => items.has(item))
}

function list(items: string[]) {
  return `[${items.join(', ')}]`
}

/** Model-route keys contain `/`, which YAML accepts unquoted; anything else odd gets quoted. */
function key(name: string) {
  return /^[\w./-]+$/.test(name) ? name : JSON.stringify(name)
}
