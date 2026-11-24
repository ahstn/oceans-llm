import type { ToggleableColumn } from '@/components/data/column-visibility-popover'

/**
 * Every session table column, in display order. The table's renderers are a `Record` over these
 * keys, so a column cannot be rendered without a toggle entry, or toggled without a renderer.
 */
const sessionColumnLabels = {
  started: { label: 'Started' },
  model: {
    label: 'Model',
    description: 'Requested model and the provider that served it.',
  },
  caller: {
    label: 'Caller',
    description: 'The user or service account that owns the session.',
  },
  harness: { label: 'Harness' },
  state: { label: 'State' },
  score: { label: 'Score', description: 'Score and its confidence.' },
  cost: {
    label: 'Cost',
    description: 'Priced spend so far, from the usage ledger.',
  },
  activeTime: { label: 'Active time' },
  requests: { label: 'Requests' },
  toolCalls: { label: 'Tool calls' },
  mcpCalls: { label: 'MCP calls' },
  dataQuality: { label: 'Data quality' },
} satisfies Record<string, Omit<ToggleableColumn<string>, 'key'>>

export type SessionColumnId = keyof typeof sessionColumnLabels
/** `Started` identifies a row, so it always shows; every other column can be hidden. */
export type SessionColumnKey = Exclude<SessionColumnId, 'started'>
export type VisibleSessionColumns = Record<SessionColumnKey, boolean>

export const sessionColumnIds = Object.keys(sessionColumnLabels) as SessionColumnId[]

export function sessionColumnLabel(key: SessionColumnId) {
  return sessionColumnLabels[key].label
}

export const toggleableSessionColumns: ReadonlyArray<ToggleableColumn<SessionColumnKey>> =
  sessionColumnIds
    .filter((key): key is SessionColumnKey => key !== 'started')
    .map((key) => ({ key, ...sessionColumnLabels[key] }))

/** State and score rarely change what an operator scans for, so they start hidden. */
const hiddenByDefault: ReadonlySet<SessionColumnKey> = new Set(['state', 'score'])

export const defaultVisibleSessionColumns = Object.fromEntries(
  toggleableSessionColumns.map((column) => [column.key, !hiddenByDefault.has(column.key)]),
) as VisibleSessionColumns
