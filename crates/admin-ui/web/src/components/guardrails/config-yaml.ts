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
  redaction: { enabled: boolean; tiers: string[] }
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
    },
  }
}

/**
 * Renders the `guardrails:` YAML for one scope. The default block is written in full; an override
 * lists only what differs from the default, because the gateway inherits every unset field.
 */
export function scopeYaml(scope: PolicyScope, draft: PolicyDraft, base: GuardrailPolicy) {
  const global = scope.kind === 'global'
  const fields: string[] = []
  const add = (changed: boolean, line: string) => {
    if (global || changed) fields.push(line)
  }
  add(draft.enabled !== base.enabled, `enabled: ${draft.enabled}`)
  add(draft.mode !== base.mode, `mode: ${draft.mode}`)
  add(!sameSet(draft.packs, base.packs), `packs: ${list(draft.packs)}`)
  add(
    !sameSet(draft.managed_checks, base.managed_checks),
    `managed_checks: ${list(draft.managed_checks)}`,
  )
  if (global) fields.push(`stream_buffer_bytes: ${base.stream_buffer_bytes}`)

  const redaction: string[] = []
  const baseRedaction = base.secret_redaction
  if (global || draft.redaction.enabled !== baseRedaction.enabled)
    redaction.push(`enabled: ${draft.redaction.enabled}`)
  if (global || !sameSet(draft.redaction.tiers, baseRedaction.tiers))
    redaction.push(`tiers: ${list(draft.redaction.tiers)}`)
  if (global) redaction.push(`disabled_rules: ${list(baseRedaction.disabled_rules)}`)
  if (redaction.length) fields.push('secret_redaction:', ...redaction.map((line) => `  ${line}`))

  if (scope.kind === 'global')
    return ['guardrails:', '  default:', ...fields.map((line) => `    ${line}`), ''].join('\n')
  const header = ['guardrails:', `  ${scope.kind === 'model' ? 'model_routes' : 'mcp_servers'}:`]
  if (!fields.length) return [...header, `    ${key(scope.name)}: {}`, ''].join('\n')
  return [...header, `    ${key(scope.name)}:`, ...fields.map((line) => `      ${line}`), ''].join(
    '\n',
  )
}

function sameSet(a: string[], b: string[]) {
  return a.length === b.length && a.every((item) => b.includes(item))
}

function list(items: string[]) {
  return `[${items.join(', ')}]`
}

/** Model-route keys contain `/`, which YAML accepts unquoted; anything else odd gets quoted. */
function key(name: string) {
  return /^[\w./-]+$/.test(name) ? name : JSON.stringify(name)
}
