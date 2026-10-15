import type {
  RateLimitMetric,
  RateLimitOperation,
  RateLimitPeriod,
  RateLimitPolicyInput,
  RateLimitPolicyView,
  RateLimitScopeKind,
} from '@/types/api'

/** Select value for "every principal of this kind" and "any model/server". */
export const ANY = '__any__'

export const OPERATION_LABELS: Record<RateLimitOperation, string> = {
  inference: 'Inference requests',
  count_tokens: 'Token counts',
  mcp_tool_call: 'MCP tool calls',
}

export const SCOPE_LABELS: Record<RateLimitScopeKind, string> = {
  global: 'Whole gateway',
  api_key: 'API key',
  user: 'User',
  team: 'Team',
}

const EACH_SCOPE_LABELS: Record<Exclude<RateLimitScopeKind, 'global'>, string> = {
  api_key: 'Each API key',
  user: 'Each user',
  team: 'Each team',
}

export const PERIOD_LABELS: Record<RateLimitPeriod, string> = {
  minute: 'Per minute (sliding)',
  hour: 'Per hour (sliding)',
  day: 'Per day (UTC quota)',
  month: 'Per month (UTC quota)',
}

const PERIOD_UNITS: Record<RateLimitPeriod, string> = {
  minute: 'minute',
  hour: 'hour',
  day: 'day',
  month: 'month',
}

export type PolicyFormState = {
  name: string
  operation: RateLimitOperation
  scopeKind: RateLimitScopeKind
  scopeId: string
  modelKey: string
  mcpServerId: string
  metric: RateLimitMetric
  period: RateLimitPeriod
  limit: string
  enabled: boolean
}

export function emptyPolicyForm(): PolicyFormState {
  return {
    name: '',
    operation: 'inference',
    scopeKind: 'api_key',
    scopeId: ANY,
    modelKey: ANY,
    mcpServerId: ANY,
    metric: 'requests',
    period: 'minute',
    limit: '60',
    enabled: true,
  }
}

export function policyFormFromView(policy: RateLimitPolicyView): PolicyFormState {
  return {
    name: policy.name,
    operation: policy.operation,
    scopeKind: policy.scope_kind,
    scopeId: policy.scope_id ?? ANY,
    modelKey: policy.model_key ?? ANY,
    mcpServerId: policy.mcp_server_id ?? ANY,
    metric: policy.metric,
    period: policy.period,
    limit: String(policy.limit),
    enabled: policy.enabled,
  }
}

/** Drops filters that do not apply to the chosen operation and scope. */
export function policyInputFromForm(form: PolicyFormState): RateLimitPolicyInput {
  const optional = (value: string) => (value === ANY ? null : value)
  return {
    name: form.name.trim(),
    operation: form.operation,
    scope_kind: form.scopeKind,
    scope_id: form.scopeKind === 'global' ? null : optional(form.scopeId),
    model_key: form.operation === 'mcp_tool_call' ? null : optional(form.modelKey),
    mcp_server_id: form.operation === 'mcp_tool_call' ? optional(form.mcpServerId) : null,
    metric: form.operation === 'inference' ? form.metric : 'requests',
    period: form.period,
    limit: Number(form.limit),
    enabled: form.enabled,
  }
}

export function limitError(form: Pick<PolicyFormState, 'limit'>): string | null {
  const limit = Number(form.limit)
  if (!Number.isInteger(limit) || limit <= 0) return 'Limit must be a whole number above 0.'
  return null
}

export function nameError(form: Pick<PolicyFormState, 'name'>): string | null {
  return form.name.trim() ? null : 'Enter a name.'
}

export function formError(form: PolicyFormState): string | null {
  return nameError(form) ?? limitError(form)
}

export function describeScope(policy: RateLimitPolicyView): string {
  if (policy.scope_kind === 'global') return SCOPE_LABELS.global
  if (!policy.scope_id) return EACH_SCOPE_LABELS[policy.scope_kind]
  return `${SCOPE_LABELS[policy.scope_kind]}: ${policy.scope_label ?? policy.scope_id}`
}

export function describeTarget(policy: RateLimitPolicyView): string {
  const operation = OPERATION_LABELS[policy.operation]
  if (policy.model_key) return `${operation} · ${policy.model_key}`
  if (policy.operation === 'mcp_tool_call') {
    return `${operation} · ${policy.mcp_server_key ?? 'any server'}`
  }
  return operation
}

export function describeLimit(policy: Pick<RateLimitPolicyView, 'limit' | 'metric' | 'period'>) {
  const unit = policy.metric === 'tokens' ? 'tokens' : 'requests'
  return `${policy.limit.toLocaleString('en-US')} ${unit} / ${PERIOD_UNITS[policy.period]}`
}

export function eachScopeLabel(scopeKind: Exclude<RateLimitScopeKind, 'global'>) {
  return EACH_SCOPE_LABELS[scopeKind]
}
