import type { ToggleableColumn } from '@/components/data/column-visibility-popover'

/**
 * Every session table column, in display order. The table's renderers are a `Record` over these
 * keys, so a column cannot be rendered without a toggle entry, or toggled without a renderer.
 */
const sessionColumnLabels = {
  started: { label: 'Started' },
  harness: { label: 'Harness' },
  model: { label: 'Model', description: 'Model, operation, and caller class.' },
  state: { label: 'State' },
  score: { label: 'Score', description: 'Score and its confidence.' },
  cost: { label: 'Cost' },
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

export const defaultVisibleSessionColumns = Object.fromEntries(
  toggleableSessionColumns.map((column) => [column.key, true]),
) as VisibleSessionColumns
