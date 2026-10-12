import type { GuardrailDecisionView, GuardrailPoliciesView } from '@/types/api'

export type GuardrailPolicy = GuardrailPoliciesView['default']
export type DecisionAction = 'allow' | 'audit' | 'deny' | 'transformed'
export type ActionBadgeVariant = 'success' | 'warning' | 'destructive' | 'outline'

/** Phases in the order a request meets them, so views can lay decisions out as a pipeline. */
export const PHASES = [
  { id: 'prompt', label: 'Prompt' },
  { id: 'model_response', label: 'Model response' },
  { id: 'generated_tool_call', label: 'Generated tool call' },
  { id: 'mcp_call', label: 'MCP call' },
  { id: 'mcp_result', label: 'MCP result' },
  { id: 'harness_pre_tool', label: 'Harness pre-tool' },
] as const

export const ACTIONS: { id: DecisionAction; label: string }[] = [
  { id: 'allow', label: 'Allow' },
  { id: 'audit', label: 'Audit' },
  { id: 'transformed', label: 'Transformed' },
  { id: 'deny', label: 'Deny' },
]

const ACTION_VARIANT: Record<string, ActionBadgeVariant> = {
  allow: 'success',
  audit: 'warning',
  transformed: 'warning',
  deny: 'destructive',
}

/** Fill classes for meters and timeline dots; badges carry the same tones through variants. */
export const ACTION_FILL: Record<string, string> = {
  allow: 'bg-success',
  audit: 'bg-warning',
  transformed: 'bg-chart-2',
  deny: 'bg-destructive',
}

const timeFormatter = new Intl.DateTimeFormat('en-US', {
  dateStyle: 'medium',
  timeStyle: 'medium',
  timeZone: 'UTC',
})

export function actionVariant(action: string): ActionBadgeVariant {
  return ACTION_VARIANT[action] ?? 'outline'
}

export function phaseLabel(phase: string) {
  return PHASES.find((item) => item.id === phase)?.label ?? phase
}

export function actionLabel(action: string) {
  return ACTIONS.find((item) => item.id === action)?.label ?? action
}

export function formatTimestamp(value: string) {
  return `${timeFormatter.format(new Date(value))} UTC`
}

export function formatLatency(micros: number) {
  return micros >= 1_000 ? `${(micros / 1_000).toFixed(1)} ms` : `${micros} µs`
}

export function ruleLabel(decision: GuardrailDecisionView) {
  return [decision.pack_id, decision.rule_id].filter(Boolean).join(' / ') || '—'
}

export function countBy<T>(items: T[], key: (item: T) => string) {
  const counts = new Map<string, number>()
  for (const item of items) counts.set(key(item), (counts.get(key(item)) ?? 0) + 1)
  return counts
}

/** Nearest-rank percentile; an empty page reports zero rather than NaN. */
export function percentile(values: number[], p: number) {
  if (values.length === 0) return 0
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)]
}

export type PolicyScope = {
  /** Same string as `GuardrailDecisionView.effective_scope`, e.g. `model_route:fast`. */
  key: string
  kind: 'global' | 'model' | 'mcp'
  name: string
  policy: GuardrailPolicy
}

export function policyScopes(policies: GuardrailPoliciesView): PolicyScope[] {
  return [
    {
      key: policies.default.scope,
      kind: 'global',
      name: 'Global default',
      policy: policies.default,
    },
    ...Object.entries(policies.model_routes).map(([name, policy]) => ({
      key: policy.scope,
      kind: 'model' as const,
      name,
      policy,
    })),
    ...Object.entries(policies.mcp_servers).map(([name, policy]) => ({
      key: policy.scope,
      kind: 'mcp' as const,
      name,
      policy,
    })),
  ]
}
