import type { AuditLogView } from '@/types/api'

const timeFormatter = new Intl.DateTimeFormat('en-GB', {
  dateStyle: 'medium',
  timeStyle: 'medium',
  timeZone: 'UTC',
})

export function formatAuditTimestamp(value: string) {
  return `${timeFormatter.format(new Date(value))} UTC`
}

/** `mcp_server` → `MCP server`, for target type labels. */
export function targetTypeLabel(targetType: string) {
  const words = targetType.split('_').map((word) => (word === 'mcp' ? 'MCP' : word))
  const label = words.join(' ')
  return label.charAt(0).toUpperCase() + label.slice(1)
}

/** `api_key.secret_revealed` → `Secret revealed`; the target type is shown beside it. */
export function actionVerbLabel(action: string) {
  const verb = action.split('.').slice(1).join('.') || action
  const label = verb.replaceAll('_', ' ')
  return label.charAt(0).toUpperCase() + label.slice(1)
}

type ActionBadgeVariant = 'destructive' | 'success' | 'secondary'

const DESTRUCTIVE_VERBS = new Set([
  'revoked',
  'deactivated',
  'disabled',
  'deleted',
  'member_removed',
])
const CREATIVE_VERBS = new Set(['created', 'provisioned', 'reactivated', 'members_added'])

export function actionVariant(action: string): ActionBadgeVariant {
  const verb = action.split('.').slice(1).join('.')
  if (DESTRUCTIVE_VERBS.has(verb)) return 'destructive'
  if (CREATIVE_VERBS.has(verb)) return 'success'
  return 'secondary'
}

export function actorLabel(entry: AuditLogView) {
  if (entry.actor.label) return entry.actor.label
  if (entry.actor.kind === 'system') return 'System'
  return entry.actor.user_id ?? 'Unknown actor'
}

export function actorKindLabel(kind: string) {
  if (kind === 'api_key') return 'API key'
  if (kind === 'system') return 'System'
  return 'Session'
}
