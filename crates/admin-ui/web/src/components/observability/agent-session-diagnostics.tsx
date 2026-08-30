import { useState, type ReactNode } from 'react'
import { ArrowRight01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import type { AgentSessionDetailView } from '@/types/api'
import {
  getAgentSessionToolMetricAvailability,
  type MetricAvailability,
} from './agent-session-metrics'

type AgentSessionDiagnosticsProps = {
  detail: AgentSessionDetailView
  formatCost: (value?: number | null) => string
  formatDuration: (value?: number | null) => string
}

const tokenCountFormatter = new Intl.NumberFormat('en-GB')

export function AgentSessionDiagnostics({
  detail,
  formatCost,
  formatDuration,
}: AgentSessionDiagnosticsProps) {
  const report = detail.report
  const components = report?.components
  const diagnostics = report?.diagnostics
  const enabledMetrics = diagnostics?.enabled_metrics
  const tokenMetricsMeasured =
    diagnostics !== undefined &&
    [
      diagnostics.token_and_cache.fresh_input_tokens,
      diagnostics.token_and_cache.cache_read_tokens,
      diagnostics.token_and_cache.cache_creation_tokens,
      diagnostics.token_and_cache.output_tokens,
    ].some((value) => value !== null)
  const toolAvailability = getAgentSessionToolMetricAvailability(detail)

  return (
    <>
      <DiagnosticSection title="Session identity">
        <DiagnosticRow label="Model" value={detail.session.requested_model_key} />
        <DiagnosticRow label="Harness" value={detail.session.harness_label ?? 'Unknown'} />
        <DiagnosticRow label="Gateway analysis session ID" value={detail.session.session_id} />
        <DiagnosticRow
          label="External session source"
          value={detail.session.session_source_observed ? 'Observed' : 'Not observed'}
        />
        <DiagnosticRow
          label="External session source hash"
          value={detail.session.session_source_hash ?? 'Not available'}
        />
      </DiagnosticSection>

      <DiagnosticSection title="Score components">
        <DiagnosticRow
          label="Cost efficiency"
          value={formatBasisPoints(components?.cost_efficiency_basis_points)}
        />
        <DiagnosticRow label="Elapsed time" value={formatDuration(components?.wall_time_ms)} />
      </DiagnosticSection>

      <DiagnosticSection
        title="Token and cache use"
        availability={metricAvailability(
          enabledMetrics ? enabledMetrics.token_metrics || enabledMetrics.cache_metrics : undefined,
          tokenMetricsMeasured,
        )}
      >
        <DiagnosticRow
          label="Fresh input tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.fresh_input_tokens)}
        />
        <DiagnosticRow
          label="Cache read tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.cache_read_tokens)}
        />
        <DiagnosticRow
          label="Cache creation tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.cache_creation_tokens)}
        />
        <DiagnosticRow
          label="Total input tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.total_input_tokens)}
        />
        <DiagnosticRow
          label="Cache read cost"
          value={formatScaledCost(diagnostics?.token_and_cache.cache_read_cost_10000, formatCost)}
        />
        <DiagnosticRow
          label="Cache write cost"
          value={formatScaledCost(
            diagnostics?.token_and_cache.cache_creation_cost_10000,
            formatCost,
          )}
        />
        <DiagnosticRow
          label="Output tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.output_tokens)}
        />
        <DiagnosticRow
          label="Reasoning tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.reasoning_tokens)}
        />
        <DiagnosticRow
          label="Visible output tokens"
          value={formatTokenCount(diagnostics?.token_and_cache.visible_output_tokens)}
        />
        <DiagnosticRow
          label="Cache read to write ratio"
          value={formatBasisPoints(
            diagnostics?.token_and_cache.cache_read_write_ratio_basis_points,
          )}
        />
        <DiagnosticRow
          label="Cache write amplification"
          value={formatBasisPoints(
            diagnostics?.token_and_cache.cache_write_amplification_basis_points,
          )}
        />
        <DiagnosticRow
          label="Cache threshold misses"
          value={formatNullable(
            diagnostics?.token_and_cache.silent_cache_threshold_miss_requests,
            'Not measured',
          )}
        />
        <DiagnosticRow
          label="Provider/model switches"
          value={formatNullable(
            diagnostics?.token_and_cache.provider_model_switches ??
              diagnostics?.token_and_cache.cache_key_switches,
          )}
        />
        <DiagnosticRow
          label="Reasoning configuration switches"
          value={formatNullable(
            diagnostics?.token_and_cache.reasoning_config_switches,
            'Not measured',
          )}
        />
        <DiagnosticRow
          label="Cache savings"
          value={formatBasisPoints(diagnostics?.token_and_cache.cache_savings_basis_points)}
        />
        <DiagnosticRow
          label="Cost without cache reads"
          value={formatScaledCost(
            diagnostics?.token_and_cache.uncached_input_cost_10000,
            formatCost,
          )}
        />
        <DiagnosticRow
          label="Pricing policy versions"
          value={
            diagnostics?.token_and_cache.pricing_policy_versions.join(' · ') || 'Not available'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection
        title="Tools and changes"
        availability={toolAvailability}
        deferredChildren={() =>
          diagnostics?.tools_and_changes.tool_servers.map((server) => (
            <DiagnosticRow
              key={server.server_key}
              label={`Tool server · ${server.server_key}`}
              value={`${server.invoked_tool_definitions} of ${server.exposed_tool_definitions} tools invoked · ${server.failed_count} failed · ${formatTokenCount(server.schema_token_estimate_per_request)} schema tokens per request · ${formatScaledCost(server.estimated_uncached_schema_cost_10000, formatCost)} without cache`}
            />
          ))
        }
      >
        <DiagnosticRow
          label="Tool calls"
          value={
            diagnostics
              ? `${diagnostics.tools_and_changes.classified_tool_calls} identified of ${diagnostics.tools_and_changes.observed_tool_calls} observed`
              : 'Not available'
          }
        />
        <DiagnosticRow
          label="Direct MCP calls"
          value={formatTokenCount(diagnostics?.tools_and_changes.direct_mcp_calls)}
        />
        <DiagnosticRow
          label="Available tools"
          value={formatTokenCount(diagnostics?.tools_and_changes.supplied_tool_definitions)}
        />
        <DiagnosticRow
          label="Tool schema size"
          value={formatBytes(diagnostics?.tools_and_changes.supplied_tool_schema_bytes)}
        />
        <DiagnosticRow
          label="Distinct file identifiers"
          value={formatNullable(diagnostics?.tools_and_changes.unique_opaque_files)}
        />
        <DiagnosticRow
          label="File activity"
          value={
            diagnostics
              ? `${diagnostics.tools_and_changes.file_reads_suspected} read · ${diagnostics.tools_and_changes.file_searches_suspected} search · ${diagnostics.tools_and_changes.file_edits_suspected} edit · ${diagnostics.tools_and_changes.file_creates_suspected} create · ${diagnostics.tools_and_changes.file_overwrites_suspected} overwrite`
              : 'Not available'
          }
        />
        <DiagnosticRow
          label="Possible rework periods"
          value={formatNullable(diagnostics?.tools_and_changes.rework_spans_suspected)}
        />
        <DiagnosticRow
          label="Verification events"
          value={formatNullable(diagnostics?.tools_and_changes.verification_results_classified)}
        />
      </DiagnosticSection>

      <DiagnosticSection
        title="Prompt context"
        availability={metricAvailability(
          enabledMetrics?.context_metrics,
          diagnostics?.context.maximum_prompt_tokens !== null &&
            diagnostics?.context.maximum_prompt_tokens !== undefined,
        )}
      >
        <DiagnosticRow
          label="Initial prompt tokens"
          value={formatTokenCount(diagnostics?.context.initial_prompt_tokens)}
        />
        <DiagnosticRow
          label="Median prompt tokens"
          value={formatTokenCount(diagnostics?.context.median_prompt_tokens)}
        />
        <DiagnosticRow
          label="Maximum prompt tokens"
          value={formatTokenCount(diagnostics?.context.maximum_prompt_tokens)}
        />
        <DiagnosticRow
          label="Configured input boundary"
          value={formatTokenCount(diagnostics?.context.input_boundary_tokens)}
        />
        <DiagnosticRow
          label="Reserved output capacity"
          value={formatTokenCount(diagnostics?.context.reserved_output_tokens)}
        />
        <DiagnosticRow
          label="Context score penalty"
          value={
            diagnostics ? `${diagnostics.context.score_penalty_points} points` : 'Not measured'
          }
        />
        <DiagnosticRow
          label="Prompt token growth per turn"
          value={formatTokenCount(diagnostics?.context.prompt_growth_per_turn)}
        />
        <DiagnosticRow
          label="Possible context compactions"
          value={formatNullable(diagnostics?.context.suspected_compactions)}
        />
      </DiagnosticSection>
    </>
  )
}

function metricAvailability(enabled: boolean | undefined, measured: boolean): MetricAvailability {
  if (enabled === false) return 'disabled'
  return measured ? 'measured' : 'unknown'
}

export function DiagnosticSection({
  title,
  summary,
  availability = 'measured',
  defaultOpen = false,
  deferredChildren,
  children,
}: {
  title: string
  summary?: string
  availability?: MetricAvailability
  defaultOpen?: boolean
  deferredChildren?: () => ReactNode
  children: ReactNode
}) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <Collapsible open={open} onOpenChange={setOpen} className="overflow-hidden border-x">
      <h3>
        <CollapsibleTrigger className="group hover:bg-muted/40 focus-visible:ring-ring flex w-full items-center gap-3 px-4 py-3 text-left transition-colors focus-visible:ring-2 focus-visible:outline-none focus-visible:ring-inset">
          <span className="font-medium">{title}</span>
          {summary ? (
            <Badge variant="outline" className="tabular-nums">
              {summary}
            </Badge>
          ) : null}
          <AppIcon
            icon={ArrowRight01Icon}
            size={14}
            stroke={1.5}
            className="text-muted-foreground ml-auto transition-transform group-data-[state=open]:rotate-90"
            aria-hidden
          />
        </CollapsibleTrigger>
      </h3>
      <CollapsibleContent>
        <div className="space-y-3 border-t px-4 py-4">
          {availability === 'disabled' ? (
            <p className="text-muted-foreground text-sm">Disabled by the analysis configuration.</p>
          ) : availability === 'unknown' ? (
            <p className="text-muted-foreground text-sm">
              Unknown — the required telemetry was not measured for this session.
            </p>
          ) : (
            <>
              {children}
              {open ? deferredChildren?.() : null}
            </>
          )}
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}

function DiagnosticRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start justify-between gap-4 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="max-w-full min-w-0 text-right font-medium break-all">{value}</span>
    </div>
  )
}

function formatBasisPoints(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : `${(value / 100).toFixed(1)}%`
}

function formatTokenCount(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : tokenCountFormatter.format(value)
}

function formatBytes(value?: number | null) {
  if (value === null || value === undefined) return 'Not available'
  if (value < 1_024) return `${value} B`
  return `${(value / 1_024).toFixed(1)} KiB`
}

function formatScaledCost(
  value: number | null | undefined,
  formatCost: (value?: number | null) => string,
) {
  return value === null || value === undefined ? 'Not available' : formatCost(value / 10_000)
}

function formatNullable(value: unknown, fallback = '—') {
  if (value === null || value === undefined) return fallback
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
    return String(value)
  }
  try {
    return JSON.stringify(value) ?? fallback
  } catch {
    return fallback
  }
}
