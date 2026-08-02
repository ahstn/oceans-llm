import { useState, type ReactNode } from 'react'
import { ArrowRight01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import type { AgentSessionDetailView } from '@/types/api'

export type MetricAvailability = 'measured' | 'unknown' | 'disabled'

type AgentSessionDiagnosticsProps = {
  detail: AgentSessionDetailView
  formatCost: (value?: number | null) => string
  formatDuration: (value?: number | null) => string
  formatTimestamp: (value: string) => string
  humanize: (value?: string | null) => string
}

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

  return metricAvailability(diagnostics?.enabled_metrics.tool_metrics, measured)
}

export function AgentSessionDiagnostics({
  detail,
  formatCost,
  formatDuration,
  formatTimestamp,
  humanize,
}: AgentSessionDiagnosticsProps) {
  const report = detail.report
  const components = report?.components
  const outcome = components?.outcome
  const coverage = detail.coverage
  const telemetryCoverage = report?.coverage
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
        <DiagnosticRow label="Operation" value={humanize(detail.session.operation)} />
        <DiagnosticRow label="Caller class" value={humanize(detail.session.caller_class)} />
        <DiagnosticRow label="Harness" value={detail.session.harness_label ?? 'Unknown'} />
        <DiagnosticRow label="Session ID" value={detail.session.session_id} />
        <DiagnosticRow
          label="External session ID"
          value={detail.session.external_session_id ?? 'Not observed'}
        />
      </DiagnosticSection>

      <DiagnosticSection title="Score components">
        <DiagnosticRow
          label="Outcome factor"
          value={formatBasisPoints(outcome?.factor_basis_points)}
        />
        <DiagnosticRow
          label="Cost efficiency"
          value={formatBasisPoints(components?.cost_efficiency_basis_points)}
        />
        <DiagnosticRow
          label="Active-time efficiency"
          value={formatBasisPoints(components?.active_time_efficiency_basis_points)}
        />
        <DiagnosticRow
          label="Excluded long gaps"
          value={formatDuration(components?.excluded_gap_time_ms)}
        />
        <DiagnosticRow
          label="Total request and MCP time"
          value={formatDuration(components?.summed_work_time_ms)}
        />
        <DiagnosticRow label="Elapsed time" value={formatDuration(components?.wall_time_ms)} />
        <DiagnosticRow
          label="Overlapping work time"
          value={formatDuration(components?.overlap_savings_ms)}
        />
        <DiagnosticRow
          label="Unclassified wait time"
          value={formatDuration(components?.unknown_wait_time_ms)}
        />
      </DiagnosticSection>

      <DiagnosticSection title="Score confidence and comparison data">
        <DiagnosticRow label="Confidence" value={humanize(detail.session.score_confidence)} />
        <DiagnosticRow label="Score status" value={humanize(detail.session.score_maturity)} />
        <DiagnosticRow
          label="Comparison group"
          value={formatNullable(components?.cohort_version, 'No comparison group')}
        />
        <DiagnosticRow
          label="Comparison fallback level"
          value={formatNullable(components?.cohort_fallback_level)}
        />
        <DiagnosticRow
          label="Sessions in comparison"
          value={formatNullable(components?.cohort_sample_size)}
        />
        <DiagnosticRow
          label="Overall telemetry coverage"
          value={formatPercent(telemetryCoverage?.overall_percent)}
        />
        <DiagnosticRow
          label="Outcome coverage"
          value={formatPercent(telemetryCoverage?.outcome_percent)}
        />
        <DiagnosticRow
          label="Cost coverage"
          value={formatPercent(telemetryCoverage?.cost_percent)}
        />
        <DiagnosticRow
          label="Timing coverage"
          value={formatPercent(telemetryCoverage?.timing_percent)}
        />
        <DiagnosticRow
          label="Payload coverage"
          value={formatPercent(telemetryCoverage?.payload_percent)}
        />
        <DiagnosticRow
          label="Comparison coverage"
          value={formatPercent(telemetryCoverage?.cohort_percent)}
        />
        <DiagnosticRow
          label="Source data"
          value={
            coverage
              ? [
                  coverage.request_metadata ? 'Request metadata' : null,
                  coverage.response_payload ? 'Response payload' : null,
                  coverage.response_payload_truncated ? 'Response payload is incomplete' : null,
                ]
                  .filter(Boolean)
                  .join(' · ') || 'Source data is not available'
              : 'Source data is not available'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection
        title="Token and cache use"
        availability={metricAvailability(
          enabledMetrics
            ? enabledMetrics.token_metrics || enabledMetrics.cache_metrics
            : undefined,
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
          label="Cache creation by lifetime"
          value={
            diagnostics
              ? [
                  `${formatTokenCount(diagnostics.token_and_cache.cache_creation_5m_tokens)} at 5 minutes`,
                  `${formatTokenCount(diagnostics.token_and_cache.cache_creation_30m_tokens)} at 30 minutes`,
                  `${formatTokenCount(diagnostics.token_and_cache.cache_creation_1h_tokens)} at 1 hour`,
                ].join(' · ')
              : 'Not measured'
          }
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
          label="Provider or model cache-key switches"
          value={formatNullable(diagnostics?.token_and_cache.cache_key_switches)}
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
              value={`${server.invoked_tool_definitions} of ${server.exposed_tool_definitions} tools invoked · ${server.failed_count} failed · ${formatTokenCount(server.schema_token_estimate_per_request)} schema tokens per request`}
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
        title="Reliability and retries"
        availability={metricAvailability(
          enabledMetrics?.reliability_metrics,
          (diagnostics?.reliability.attempt_coverage_percent ?? 0) > 0 ||
            (diagnostics?.reliability.tool_invocations ?? 0) > 0,
        )}
        deferredChildren={() =>
          diagnostics?.reliability.tools.map((tool) => (
            <DiagnosticRow
              key={`${tool.server_key ?? 'local'}:${tool.tool_key}`}
              label={`Tool · ${tool.server_key ? `${tool.server_key} / ` : ''}${tool.tool_key}`}
              value={`${tool.failed_count} failed of ${tool.invocation_count} · ${formatDuration(tool.latency_ms)}${tool.post_error_input_tokens === null ? '' : ` · ${formatTokenCount(tool.post_error_input_tokens)} input tokens after errors`}`}
            />
          ))
        }
      >
        <DiagnosticRow
          label="Attempt coverage"
          value={formatPercent(diagnostics?.reliability.attempt_coverage_percent)}
        />
        <DiagnosticRow
          label="Wasted attempts"
          value={
            diagnostics
              ? `${diagnostics.reliability.wasted_attempts} of ${diagnostics.reliability.total_attempts} attempts · ${formatDuration(diagnostics.reliability.wasted_attempt_latency_ms)}`
              : 'Not measured'
          }
        />
        <DiagnosticRow
          label="Fallback attempts"
          value={formatNullable(diagnostics?.reliability.fallback_attempts)}
        />
        <DiagnosticRow
          label="Tool reliability"
          value={
            diagnostics
              ? `${diagnostics.reliability.failed_tool_invocations} failures · ${diagnostics.reliability.truncated_tool_results} truncated results · ${diagnostics.reliability.tool_invocations} calls`
              : 'Not measured'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection
        title="Skills"
        availability={metricAvailability(
          enabledMetrics?.skill_metrics,
          (diagnostics?.skills.instrumented_request_count ?? 0) > 0,
        )}
        deferredChildren={() =>
          diagnostics?.skills.items.map((skill) => (
            <DiagnosticRow
              key={skill.name}
              label={`Skill · ${skill.name}`}
              value={`${skill.used_request_count} used of ${skill.available_request_count} available requests · ${skill.abandoned_request_count} abandoned`}
            />
          ))
        }
      >
        <DiagnosticRow
          label="Skill use"
          value={
            diagnostics?.skills.available_skill_count === null
              ? 'Not measured'
              : `${diagnostics?.skills.used_skill_count ?? 0} used · ${diagnostics?.skills.unused_skill_count ?? 0} unused · ${diagnostics?.skills.available_skill_count ?? 0} available`
          }
        />
        <DiagnosticRow
          label="Skill token load"
          value={
            diagnostics
              ? `${formatTokenCount(diagnostics.skills.description_tokens_per_request)} descriptions per request · ${formatTokenCount(diagnostics.skills.loaded_body_tokens)} bodies · ${formatTokenCount(diagnostics.skills.loaded_resource_tokens)} resources`
              : 'Not measured'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection
        title="Outcome evidence"
        availability={metricAvailability(
          enabledMetrics?.outcome_metrics,
          (diagnostics?.outcome.file_signal_coverage_percent ?? 0) > 0,
        )}
      >
        <DiagnosticRow
          label="File-signal coverage"
          value={formatPercent(diagnostics?.outcome.file_signal_coverage_percent)}
        />
        <DiagnosticRow
          label="Cost per file touched"
          value={formatScaledCost(diagnostics?.outcome.cost_per_file_touched_10000, formatCost)}
        />
        <DiagnosticRow
          label="Cost per successful session"
          value={formatScaledCost(
            diagnostics?.outcome.cost_per_successful_session_10000,
            formatCost,
          )}
        />
        <DiagnosticRow
          label="Rework ratio"
          value={formatBasisPoints(diagnostics?.outcome.rework_ratio_basis_points)}
        />
        <DiagnosticRow
          label="Verification rate"
          value={formatBasisPoints(diagnostics?.outcome.verification_rate_basis_points)}
        />
        <DiagnosticRow
          label="Repeated file activity"
          value={
            diagnostics?.outcome.repeated_file_interactions_suspected === null
              ? 'Not measured'
              : `${diagnostics?.outcome.repeated_file_interactions_suspected ?? 0} repeated interactions across ${diagnostics?.outcome.files_with_repeated_interactions_suspected ?? 0} files`
          }
        />
        <DiagnosticRow
          label="Zero detected outcome"
          value={
            diagnostics?.outcome.zero_outcome === null
              ? 'Not measured'
              : diagnostics?.outcome.zero_outcome
                ? 'Yes'
                : 'No'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection
        title="Finish reasons"
        availability={metricAvailability(
          enabledMetrics?.finish_reason_metrics,
          (diagnostics?.finish_reasons.instrumented_request_count ?? 0) > 0,
        )}
      >
        <DiagnosticRow
          label="Finish-reason coverage"
          value={
            diagnostics
              ? `${diagnostics.finish_reasons.instrumented_request_count} requests measured`
              : 'Not measured'
          }
        />
        <DiagnosticRow
          label="Length-limited requests"
          value={formatNullable(diagnostics?.finish_reasons.length_limited_requests)}
        />
        {diagnostics?.finish_reasons.items.map((item) => (
          <DiagnosticRow key={item.reason} label={humanize(item.reason)} value={item.count} />
        ))}
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
          label="P90 prompt tokens"
          value={formatTokenCount(diagnostics?.context.p90_prompt_tokens)}
        />
        <DiagnosticRow
          label="Maximum prompt tokens"
          value={formatTokenCount(diagnostics?.context.maximum_prompt_tokens)}
        />
        <DiagnosticRow
          label="Peak input utilisation"
          value={formatBasisPoints(diagnostics?.context.peak_input_utilization_basis_points)}
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
          label="Requests above the input boundary"
          value={formatNullable(diagnostics?.context.requests_over_input_boundary)}
        />
        <DiagnosticRow
          label="Context score penalty"
          value={diagnostics ? `${diagnostics.context.score_penalty_points} points` : 'Not measured'}
        />
        <DiagnosticRow
          label="Prompt token growth per turn"
          value={formatTokenCount(diagnostics?.context.prompt_growth_per_turn)}
        />
        <DiagnosticRow
          label="Prompt token growth per active minute"
          value={formatTokenCount(diagnostics?.context.prompt_growth_per_active_minute)}
        />
        <DiagnosticRow
          label="Possible context compactions"
          value={formatNullable(diagnostics?.context.suspected_compactions)}
        />
        <DiagnosticRow
          label="Possible context resets"
          value={formatNullable(diagnostics?.context.suspected_context_resets)}
        />
        <DiagnosticRow
          label="Answer verification"
          value={
            diagnostics
              ? diagnostics.semantic_verification_available
                ? 'Available'
                : 'Not available'
              : 'Not available'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection title="Analysis versions">
        <DiagnosticRow label="Report schema" value={report?.report_schema_version ?? '—'} />
        <DiagnosticRow label="Analyzer" value={report?.analyzer_version ?? '—'} />
        <DiagnosticRow label="Score policy" value={report?.score_policy_version ?? '—'} />
        <DiagnosticRow
          label="Configuration"
          value={report?.configuration_version || 'Default configuration'}
        />
        <DiagnosticRow
          label="Enabled metric groups"
          value={
            diagnostics
              ? Object.entries(diagnostics.enabled_metrics)
                  .filter(([, enabled]) => enabled)
                  .map(([name]) => humanize(name))
                  .join(' · ') || 'None'
              : 'Not available'
          }
        />
        <DiagnosticRow
          label="Boundary policy"
          value={detail.analysis?.boundary_policy_version ?? '—'}
        />
        <DiagnosticRow
          label="Observation parser"
          value={detail.analysis?.observation_parser_version ?? '—'}
        />
        <DiagnosticRow
          label="Pricing policy"
          value={detail.analysis?.pricing_policy_version ?? '—'}
        />
        <DiagnosticRow
          label="Comparison snapshot"
          value={detail.analysis?.cohort_snapshot_digest ?? '—'}
        />
        <DiagnosticRow label="Analysis ID" value={detail.analysis?.analysis_id ?? '—'} />
        <DiagnosticRow
          label="Latest input time"
          value={detail.analysis ? formatTimestamp(detail.analysis.input_watermark_at) : '—'}
        />
      </DiagnosticSection>
    </>
  )
}

function metricAvailability(
  enabled: boolean | undefined,
  measured: boolean,
): MetricAvailability {
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
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className="overflow-hidden border-x"
    >
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
      <span className="text-right font-medium">{value}</span>
    </div>
  )
}

function formatBasisPoints(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : `${(value / 100).toFixed(1)}%`
}

function formatPercent(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : `${value}%`
}

function formatTokenCount(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : value.toLocaleString()
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
  return value === null || value === undefined ? fallback : String(value)
}
