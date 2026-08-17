import type { AgentSessionDetailView } from '@/types/api'

export type MetricAvailability = 'measured' | 'unknown' | 'disabled'

export function getAgentSessionToolMetricAvailability(
  detail: AgentSessionDetailView,
): MetricAvailability {
  const diagnostics = detail.report?.diagnostics
  const measured =
    detail.observations.some(
      ({ facts }) =>
        typeof facts.supplied_tool_count === 'number' ||
        facts.supplied_tools.length > 0 ||
        typeof facts.tool_name === 'string',
    ) || (diagnostics?.tools_and_changes.observed_tool_calls ?? 0) > 0

  if (diagnostics?.enabled_metrics.tool_metrics === false) return 'disabled'
  return measured ? 'measured' : 'unknown'
}
