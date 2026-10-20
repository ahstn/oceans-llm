import { Link } from '@tanstack/react-router'

import { Button } from '@/components/ui/button'
import { formatCount } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { RequestLogDetailView, RequestLogView } from '@/types/api'

import { formatBasisPoints } from './format'

/** Tool counts in two groups: what the request offered the model, and what the model called. */
function toolCountGroups(log: RequestLogView) {
  const counts = log.tool_cardinality
  return [
    {
      label: 'Offered',
      items: [
        { label: 'MCP servers', value: counts.referenced_mcp_server_count },
        { label: 'Request tools', value: counts.request_tool_count },
        { label: 'Tools exposed', value: counts.exposed_tool_count },
        { label: 'Tools filtered', value: counts.filtered_tool_count },
      ],
    },
    {
      label: 'Called',
      items: [
        { label: 'Tool calls', value: counts.invoked_tool_count },
        { label: 'MCP tool calls', value: counts.invoked_mcp_tool_count },
        { label: 'Distinct tools called', value: counts.invoked_distinct_tool_count },
      ],
    },
  ]
}

/** All seven tool counts as two hairline strips, so no label has to wrap. */
export function ToolCounts({ log }: { log: RequestLogView }) {
  return (
    <div
      data-testid="request-log-tool-counts"
      className="grid min-w-0 gap-4 lg:grid-cols-[4fr_3fr]"
    >
      {toolCountGroups(log).map((group) => (
        <div key={group.label} className="min-w-0">
          <p className="text-muted-foreground tracking-label text-2xs mb-2 font-semibold uppercase">
            {group.label}
          </p>
          <dl
            className={cn(
              'bg-border ring-foreground/10 grid gap-px overflow-hidden rounded-lg ring-1',
              group.items.length === 4 ? 'grid-cols-2 sm:grid-cols-4' : 'grid-cols-3',
            )}
          >
            {group.items.map((item) => (
              // Column-reverse keeps the dt-before-dd order a definition list needs while the
              // count reads first.
              <div
                key={item.label}
                className="bg-card flex min-w-0 flex-col-reverse gap-0.5 px-3 py-2.5"
              >
                <dt className="text-muted-foreground truncate text-xs" title={item.label}>
                  {item.label}
                </dt>
                <dd className="text-foreground text-base leading-tight font-semibold tabular-nums">
                  {formatCount(item.value)}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      ))}
    </div>
  )
}

export function McpInvocationsLink({ requestId }: { requestId: string }) {
  return (
    <Button type="button" variant="outline" size="sm" asChild>
      <Link to="/observability/mcp-invocations" search={{ request_id: requestId }}>
        View MCP invocations
      </Link>
    </Button>
  )
}

/** The MCP definition overhead as a headline figure plus the estimator facts behind it. */
export function McpTokenOverhead({
  overhead,
}: {
  overhead: NonNullable<RequestLogDetailView['mcp_token_overhead']>
}) {
  const facts = [
    { label: 'Tools', value: formatCount(overhead.exposed_tool_count) },
    { label: 'Cache hits', value: formatCount(overhead.cache_hit_count) },
    { label: 'Cache misses', value: formatCount(overhead.cache_miss_count) },
    { label: 'Context window', value: formatCount(overhead.context_window_tokens) },
    { label: 'Confidence', value: overhead.confidence },
    { label: 'Estimator', value: overhead.estimator_source, mono: true },
  ]

  return (
    <div data-testid="request-log-mcp-overhead" className="flex min-w-0 flex-col gap-2">
      <p className="text-muted-foreground text-xs">
        MCP token overhead · context-window estimate, not spend accounting
      </p>
      <p className="text-foreground text-sm">
        <span className="text-xl font-semibold tabular-nums">
          {formatCount(overhead.estimated_definition_tokens)}
        </span>{' '}
        <span className="text-subtle-foreground">definition tokens</span>
        {overhead.context_window_tokens ? (
          <span className="text-muted-foreground">
            {' '}
            · {formatBasisPoints(overhead.context_window_percent_bps)} of a{' '}
            {formatCount(overhead.context_window_tokens)}-token context window
          </span>
        ) : null}
      </p>
      <dl className="mt-1 grid grid-cols-2 gap-x-6 gap-y-2 sm:grid-cols-3 xl:grid-cols-6">
        {facts.map((fact) => (
          <div key={fact.label} className="min-w-0">
            <dt className="text-muted-foreground text-xs">{fact.label}</dt>
            <dd
              className={cn(
                'text-foreground truncate text-sm tabular-nums',
                fact.mono && 'font-mono',
              )}
              title={fact.value}
            >
              {fact.value}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  )
}
