import type { ToggleableColumn } from '@/components/data/column-visibility-popover'

export type SessionColumnKey =
  | 'harness'
  | 'model'
  | 'state'
  | 'score'
  | 'cost'
  | 'activeTime'
  | 'requests'
  | 'toolCalls'
  | 'mcpCalls'
  | 'dataQuality'

export type VisibleSessionColumns = Record<SessionColumnKey, boolean>

/** `Started` identifies a row, so it always shows; every other column can be hidden. */
export const toggleableSessionColumns: ReadonlyArray<ToggleableColumn<SessionColumnKey>> = [
  { key: 'harness', label: 'Harness' },
  { key: 'model', label: 'Model', description: 'Model, operation, and caller class.' },
  { key: 'state', label: 'State' },
  { key: 'score', label: 'Score', description: 'Score and its confidence.' },
  { key: 'cost', label: 'Cost' },
  { key: 'activeTime', label: 'Active time' },
  { key: 'requests', label: 'Requests' },
  { key: 'toolCalls', label: 'Tool calls' },
  { key: 'mcpCalls', label: 'MCP calls' },
  { key: 'dataQuality', label: 'Data quality' },
]

export const defaultVisibleSessionColumns: VisibleSessionColumns = {
  harness: true,
  model: true,
  state: true,
  score: true,
  cost: true,
  activeTime: true,
  requests: true,
  toolCalls: true,
  mcpCalls: true,
  dataQuality: true,
}
