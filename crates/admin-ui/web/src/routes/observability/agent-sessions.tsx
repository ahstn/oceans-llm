import { useEffect, useMemo, useRef, useState, useTransition } from 'react'
import { ArrowRight01Icon } from '@hugeicons/core-free-icons'

import { Link, createFileRoute, useRouter } from '@tanstack/react-router'
import {
  getCoreRowModel,
  useReactTable,
  type ColumnDef,
  type PaginationState,
  type Updater,
} from '@tanstack/react-table'

import { AppIcon } from '@/components/icons/app-icon'
import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import { DataGrid } from '@/components/reui/data-grid/data-grid'
import { DataGridPagination } from '@/components/reui/data-grid/data-grid-pagination'
import { DataGridTable } from '@/components/reui/data-grid/data-grid-table'
import { AgentSessionDateFilter } from '@/components/reui/agent-session-date-filter'
import { Filters, type Filter, type FilterFieldConfig } from '@/components/reui/filters'
import {
  Timeline,
  TimelineContent,
  TimelineDate,
  TimelineHeader,
  TimelineIndicator,
  TimelineItem,
  TimelineSeparator,
  TimelineTitle,
} from '@/components/reui/timeline'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Skeleton } from '@/components/ui/skeleton'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getAgentSessions, getObservabilityAgentSessionDetail } from '@/server/admin-data.functions'
import type {
  AgentObservationView,
  AgentSessionDetailView,
  AgentSessionFiltersInput,
  AgentSessionRequestView,
  AgentSessionSummaryView,
} from '@/types/api'

type AgentSessionRouteSearch = AgentSessionFiltersInput & { session_id?: string }

const timestampFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
})
const currencyFormatters = {
  standard: new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }),
  precise: new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 4,
    maximumFractionDigits: 4,
  }),
}

export const Route = createFileRoute('/observability/agent-sessions')({
  validateSearch: (search: Record<string, unknown>) => normalizeSearch(search),
  loaderDeps: ({ search }) => search,
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: ({ deps }) => {
    const { session_id: _sessionId, ...filters } = deps
    return getAgentSessions({ data: filters })
  },
  component: AgentSessionsPage,
})

const sessionFilterFields = [
  'lifecycle',
  'gateway_outcome',
  'score_maturity',
  'score_confidence',
  'minimum_coverage_percent',
  'harness_key',
  'requested_model_key',
  'operation',
  'caller_class',
  'user_id',
  'team_id',
  'service_account_id',
  'session_source_id',
  'external_session_id',
  'request_tag_key',
  'request_tag_value',
] as const satisfies readonly (keyof AgentSessionFiltersInput)[]

const filterFields: FilterFieldConfig<string>[] = [
  {
    key: 'lifecycle',
    label: 'Session state',
    type: 'select',
    options: [
      { value: 'open', label: 'Open' },
      { value: 'finalized', label: 'Finalized' },
    ],
  },
  {
    key: 'gateway_outcome',
    label: 'Outcome',
    type: 'select',
    options: [
      { value: 'succeeded', label: 'Succeeded' },
      { value: 'partial', label: 'Partial' },
      { value: 'failed', label: 'Failed' },
      { value: 'unknown', label: 'Unknown' },
    ],
  },
  {
    key: 'score_maturity',
    label: 'Score maturity',
    type: 'select',
    options: [
      { value: 'experimental', label: 'Experimental' },
      { value: 'calibrated', label: 'Calibrated' },
    ],
  },
  {
    key: 'score_confidence',
    label: 'Score confidence',
    type: 'select',
    options: [
      { value: 'low', label: 'Low' },
      { value: 'medium', label: 'Medium' },
      { value: 'high', label: 'High' },
    ],
  },
  {
    key: 'minimum_coverage_percent',
    label: 'Minimum coverage',
    type: 'text',
    allowCustomValues: true,
  },
  { key: 'harness_key', label: 'Harness', type: 'text', allowCustomValues: true },
  { key: 'requested_model_key', label: 'Model', type: 'text', allowCustomValues: true },
  { key: 'operation', label: 'Operation', type: 'text', allowCustomValues: true },
  { key: 'caller_class', label: 'Caller class', type: 'text', allowCustomValues: true },
  { key: 'user_id', label: 'User ID', type: 'text', allowCustomValues: true },
  { key: 'team_id', label: 'Team ID', type: 'text', allowCustomValues: true },
  {
    key: 'service_account_id',
    label: 'Service account ID',
    type: 'text',
    allowCustomValues: true,
  },
  { key: 'session_source_id', label: 'Session source ID', type: 'text', allowCustomValues: true },
  {
    key: 'external_session_id',
    label: 'External session ID',
    type: 'text',
    allowCustomValues: true,
  },
  { key: 'request_tag_key', label: 'Request tag key', type: 'text', allowCustomValues: true },
  {
    key: 'request_tag_value',
    label: 'Request tag value',
    type: 'text',
    allowCustomValues: true,
  },
]

export function AgentSessionsPage() {
  const { data: sessionPage } = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const search = Route.useSearch()
  const router = useRouter()
  const [isListPending, startListTransition] = useTransition()
  const selectedSessionId = search.session_id ?? null
  const [selectedDetail, setSelectedDetail] = useState<AgentSessionDetailView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [detailRetry, setDetailRetry] = useState(0)
  const filterNavigationTimer = useRef<number | undefined>(undefined)
  const filterSearchKey = sessionFilterFields.map((field) => search[field] ?? '').join('\u0000')
  const [filterDraft, setFilterDraft] = useState(() => ({
    searchKey: filterSearchKey,
    filters: filtersFromSearch(search),
  }))
  const showScore = session.capabilities.calibrated_score_visible

  useEffect(() => {
    if (!selectedSessionId) {
      setSelectedDetail(null)
      setDetailPending(false)
      setDetailError(null)
      return
    }
    let cancelled = false
    setSelectedDetail(null)
    setDetailPending(true)
    setDetailError(null)
    void getObservabilityAgentSessionDetail({ data: { sessionId: selectedSessionId } })
      .then((response) => {
        if (!cancelled) setSelectedDetail(response.data)
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDetailError(error instanceof Error ? error.message : 'Failed to load agent session')
        }
      })
      .finally(() => {
        if (!cancelled) setDetailPending(false)
      })
    return () => {
      cancelled = true
    }
  }, [selectedSessionId, detailRetry])

  useEffect(
    () => () => {
      if (filterNavigationTimer.current !== undefined) {
        window.clearTimeout(filterNavigationTimer.current)
      }
    },
    [],
  )

  const columns = useMemo<ColumnDef<AgentSessionSummaryView>[]>(
    () => [
      {
        accessorKey: 'started_at',
        header: 'Started',
        cell: ({ row }) => formatTimestamp(row.original.started_at),
        size: 170,
      },
      {
        accessorKey: 'harness_label',
        header: 'Harness',
        cell: ({ row }) => {
          const harnessLabel = row.original.harness_label ?? row.original.harness_key ?? 'Unknown'
          return (
            <AgentHarnessLabel harnessKey={row.original.harness_key ?? harnessLabel}>
              {harnessLabel}
            </AgentHarnessLabel>
          )
        },
        size: 150,
      },
      {
        accessorKey: 'requested_model_key',
        header: 'Model',
        cell: ({ row }) => (
          <div>
            <p>{row.original.requested_model_key}</p>
            <p className="text-muted-foreground text-xs">
              {row.original.operation} · {humanize(row.original.caller_class)}
            </p>
          </div>
        ),
        size: 180,
      },
      {
        accessorKey: 'lifecycle',
        header: 'State',
        cell: ({ row }) => <StateBadge value={row.original.lifecycle} />,
        size: 105,
      },
      {
        accessorKey: 'efficiency_score',
        header: 'Score',
        cell: ({ row }) => (
          <div>
            <p className="font-medium tabular-nums">
              {showScore ? (row.original.efficiency_score ?? '—') : 'Score not shown'}
            </p>
            <p className="text-muted-foreground text-xs">
              {humanize(row.original.score_confidence)} confidence
            </p>
          </div>
        ),
        size: 120,
      },
      {
        accessorKey: 'normalized_cost_usd',
        header: 'Cost',
        cell: ({ row }) => formatCost(row.original.normalized_cost_usd),
        size: 100,
      },
      {
        accessorKey: 'active_time_ms',
        header: 'Active time',
        cell: ({ row }) => formatDuration(row.original.active_time_ms),
        size: 110,
      },
      {
        accessorKey: 'request_count',
        header: 'Requests',
        cell: ({ row }) => row.original.request_count.toLocaleString(),
        size: 90,
      },
      {
        accessorKey: 'tool_call_count',
        header: 'Tool calls',
        cell: ({ row }) => formatCount(row.original.tool_call_count),
        size: 90,
      },
      {
        accessorKey: 'mcp_call_count',
        header: 'MCP calls',
        cell: ({ row }) => formatCount(row.original.mcp_call_count),
        size: 90,
      },
      {
        accessorKey: 'limitations',
        header: 'Data quality',
        cell: ({ row }) =>
          row.original.limitations.length === 0 ? (
            <span className="text-muted-foreground">Complete</span>
          ) : (
            <Badge variant="outline">
              {row.original.limitations.length}{' '}
              {row.original.limitations.length === 1 ? 'data limit' : 'data limits'}
            </Badge>
          ),
        size: 140,
      },
    ],
    [showScore],
  )

  const pagination: PaginationState = {
    pageIndex: (search.page ?? 1) - 1,
    pageSize: search.page_size ?? 50,
  }
  const table = useReactTable({
    data: sessionPage.items,
    columns,
    getRowId: (row) => row.session_id,
    getCoreRowModel: getCoreRowModel(),
    manualPagination: true,
    rowCount: sessionPage.total,
    state: { pagination },
    onPaginationChange: (updater) => updatePagination(updater, pagination),
  })

  function navigate(next: AgentSessionRouteSearch) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/agent-sessions',
        search: normalizeSearch(next as Record<string, unknown>),
      })
    })
  }

  function updatePagination(updater: Updater<PaginationState>, current: PaginationState) {
    const next = typeof updater === 'function' ? updater(current) : updater
    navigate({ ...search, page: next.pageIndex + 1, page_size: next.pageSize })
  }

  function updateFilters(filters: Filter<string>[]) {
    setFilterDraft({ searchKey: filterSearchKey, filters })
    const next = { ...search, page: 1 }
    for (const field of sessionFilterFields) {
      delete next[field]
    }
    for (const filter of filters) {
      const value = filter.values[0]?.trim()
      if (value) {
        next[filter.field as keyof AgentSessionFiltersInput] = value as never
      }
    }
    if (filterNavigationTimer.current !== undefined) {
      window.clearTimeout(filterNavigationTimer.current)
    }
    filterNavigationTimer.current = window.setTimeout(() => navigate(next), 250)
  }

  function openDetail(session: AgentSessionSummaryView) {
    navigate({ ...search, session_id: session.session_id })
  }

  const activeFilters =
    filterDraft.searchKey === filterSearchKey ? filterDraft.filters : filtersFromSearch(search)
  const hasUnavailableCallMetrics = sessionPage.items.some(
    (item) =>
      item.report_schema_version != null &&
      (item.tool_call_count == null || item.mcp_call_count == null),
  )

  return (
    <main className="flex min-w-0 flex-1 flex-col gap-6">
      <header className="flex flex-col gap-2">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <p className="text-muted-foreground text-sm">Observability</p>
            <h1 className="text-2xl font-semibold tracking-tight">Agent sessions</h1>
          </div>
          <Badge variant="outline" className="tabular-nums">
            {sessionPage.total.toLocaleString()} {sessionPage.total === 1 ? 'session' : 'sessions'}
          </Badge>
        </div>
        <p className="text-muted-foreground max-w-3xl text-sm">
          Review the outcome, cost, active time, and data quality for each agent session. The system
          does not show scores until calibration is complete.
        </p>
      </header>

      <Card>
        <CardHeader className="gap-1 border-b">
          <CardTitle className="text-base">Session explorer</CardTitle>
          <CardDescription>
            Filter sessions by owner, harness, score confidence, state, or start date.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4 pt-5">
          <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
            <Filters
              filters={activeFilters}
              fields={filterFields}
              onChange={updateFilters}
              size="sm"
              enableShortcut
            />
            <div className="flex flex-wrap items-center gap-2">
              <AgentSessionDateFilter
                startedAfter={search.started_after}
                startedBefore={search.started_before}
                onChange={({ startedAfter, startedBefore }) =>
                  navigate({
                    ...search,
                    page: 1,
                    started_after: startedAfter,
                    started_before: startedBefore,
                  })
                }
              />
              {hasActiveSearch(search) ? (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => navigate({ page: 1, page_size: 50 })}
                >
                  Clear
                </Button>
              ) : null}
            </div>
          </div>

          {hasUnavailableCallMetrics ? (
            <Alert>
              <AlertTitle>Tool-call data is not available</AlertTitle>
              <AlertDescription>
                The API did not return tool-call or MCP-call counts for some analyzed sessions.
                Restart the gateway after you update it. For local demo data, run{' '}
                <code className="font-mono">mise run gateway-reset-local-demo</code>.
              </AlertDescription>
            </Alert>
          ) : null}

          <div className={isListPending ? 'opacity-60 transition-opacity' : undefined}>
            <DataGrid
              table={table}
              recordCount={sessionPage.total}
              isLoading={isListPending}
              loadingMode="skeleton"
              emptyMessage="No agent sessions match these filters."
              onRowClick={(row) => void openDetail(row)}
              tableLayout={{
                dense: true,
                rowBorder: true,
                headerBackground: false,
                headerSticky: true,
                columnsResizable: true,
                width: 'fixed',
              }}
              tableClassNames={{
                base: 'text-[var(--color-text)]',
                headerRow:
                  'bg-[color:var(--color-surface-muted)] [&>th]:font-semibold [&>th]:text-[var(--color-text-soft)]',
                bodyRow: 'transition-colors',
              }}
            >
              <div className="min-w-0 overflow-x-auto rounded-md border border-[color:var(--color-border)]">
                <DataGridTable />
              </div>
              <DataGridPagination sizes={[25, 50, 100]} />
            </DataGrid>
          </div>
        </CardContent>
      </Card>

      <Sheet
        open={selectedSessionId !== null}
        onOpenChange={(open) => {
          if (!open) {
            navigate({ ...search, session_id: undefined })
          }
        }}
      >
        <SheetContent className="overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-3xl data-[side=right]:lg:w-[50vw] data-[side=right]:lg:max-w-none data-[side=right]:lg:min-w-[50vw]">
          <SheetHeader className="border-b">
            <SheetTitle>Agent session details</SheetTitle>
            <SheetDescription className="font-mono text-xs">{selectedSessionId}</SheetDescription>
          </SheetHeader>
          <div className="pb-6">
            {detailPending ? <DetailSkeleton /> : null}
            {detailError ? (
              <Alert variant="destructive">
                <AlertTitle>Session details are not available</AlertTitle>
                <AlertDescription className="flex items-center justify-between gap-3">
                  <span>{detailError}</span>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setDetailRetry((value) => value + 1)}
                  >
                    Retry
                  </Button>
                </AlertDescription>
              </Alert>
            ) : null}
            {selectedDetail ? (
              <SessionDetail
                key={selectedDetail.session.session_id}
                detail={selectedDetail}
                showScore={showScore}
              />
            ) : null}
          </div>
        </SheetContent>
      </Sheet>
    </main>
  )
}

function SessionDetail({
  detail,
  showScore,
}: {
  detail: AgentSessionDetailView
  showScore: boolean
}) {
  const report = detail.report
  const components = report?.components
  const outcome = components?.outcome
  const coverage = detail.coverage
  const telemetryCoverage = report?.coverage
  const diagnostics = report?.diagnostics

  return (
    <>
      <div className="space-y-4 px-4 pb-4">
        <section aria-label="Session summary" className="grid grid-cols-3 divide-x border">
          <Metric
            label="Session score"
            value={showScore ? formatNullable(report?.score) : 'Score not shown'}
          />
          <Metric label="Normalised cost" value={formatCost(detail.session.normalized_cost_usd)} />
          <Metric label="Total time" value={formatDuration(components?.wall_time_ms)} />
        </section>

        {!showScore ? (
          <Alert>
            <AlertTitle>Calibration data</AlertTitle>
            <AlertDescription>
              The system does not show the session score during calibration. You can review session
              boundaries, outcomes, comparison groups, and data coverage.
            </AlertDescription>
          </Alert>
        ) : null}

        {detail.session.limitations.length > 0 ? (
          <Alert>
            <AlertTitle>Data limits</AlertTitle>
            <AlertDescription>
              {detail.session.limitations.map(formatLimitation).join(' · ')}
            </AlertDescription>
          </Alert>
        ) : null}

        {detail.request_history_truncated || detail.observation_history_truncated ? (
          <Alert>
            <AlertTitle>Some history is not shown</AlertTitle>
            <AlertDescription>
              This view shows a maximum of 1,000 requests and 1,000 detected activities. Open
              request logs to review retained request history.
            </AlertDescription>
          </Alert>
        ) : null}
      </div>
      <div className="divide-y border-y">
        <SessionEventStream
          requests={detail.requests}
          observations={detail.observations}
          historyTruncated={detail.request_history_truncated}
        />
        <ToolExposure observations={detail.observations} />

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

        <DiagnosticSection title="Token and cache use">
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
            label="Output tokens"
            value={formatTokenCount(diagnostics?.token_and_cache.output_tokens)}
          />
          <DiagnosticRow
            label="Reasoning tokens"
            value={formatTokenCount(diagnostics?.token_and_cache.reasoning_tokens)}
          />
          <DiagnosticRow
            label="Cache savings"
            value={formatBasisPoints(diagnostics?.token_and_cache.cache_savings_basis_points)}
          />
          <DiagnosticRow
            label="Cost without cache reads"
            value={formatScaledCost(diagnostics?.token_and_cache.uncached_input_cost_10000)}
          />
          <DiagnosticRow
            label="Pricing policy versions"
            value={
              diagnostics?.token_and_cache.pricing_policy_versions.join(' · ') || 'Not available'
            }
          />
        </DiagnosticSection>

        <DiagnosticSection title="Tools and changes">
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

        <DiagnosticSection title="Prompt context">
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
      </div>
    </>
  )
}

const TOOL_PREVIEW_LIMIT = 6

interface ToolExposureItem {
  name: string
  totalTokens: number
  callCount: number
}

function SessionEventStream({
  requests,
  observations,
  historyTruncated,
}: {
  requests: AgentSessionRequestView[]
  observations: AgentObservationView[]
  historyTruncated: boolean
}) {
  const [visibleCount, setVisibleCount] = useState(25)
  const visible = requests.slice(0, visibleCount)
  const observationsByRequest = useMemo(() => {
    const grouped = new Map<string, AgentObservationView[]>()
    for (const observation of observations) {
      const events = grouped.get(observation.source_request_id) ?? []
      events.push(observation)
      grouped.set(observation.source_request_id, events)
    }
    return grouped
  }, [observations])

  return (
    <DiagnosticSection
      title="Event stream"
      summary={formatRequestCountSummary(requests.length, historyTruncated)}
      defaultOpen
    >
      {requests.length > 0 ? (
        <div className="space-y-4">
          <Timeline defaultValue={visible.length}>
            {visible.map((request, index) => {
              const requestObservations = observationsByRequest.get(request.request_id) ?? []
              return (
                <TimelineItem key={request.request_id} step={index + 1}>
                  <TimelineIndicator
                    className={
                      request.terminal_success === false
                        ? 'border-destructive bg-destructive/15'
                        : 'bg-background'
                    }
                  />
                  <TimelineSeparator />
                  <Link
                    to="/observability/request-logs"
                    search={{ request_id: request.request_id }}
                    aria-label={`Open request ${request.request_id} in request logs`}
                    className="group/event hover:bg-muted/50 focus-visible:ring-ring block rounded-md px-2 py-2 transition-colors focus-visible:ring-2 focus-visible:outline-none"
                  >
                    <TimelineHeader className="flex items-start justify-between gap-3">
                      <div className="flex min-w-0 items-baseline gap-2">
                        <TimelineTitle className="shrink-0">
                          Request {request.ordinal + 1}
                        </TimelineTitle>
                        <p className="text-muted-foreground truncate font-mono text-xs">
                          {request.request_id}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        <TimelineDate
                          dateTime={request.occurred_at}
                          className="mb-0 font-normal tabular-nums"
                        >
                          {formatTimestamp(request.occurred_at)}
                        </TimelineDate>
                        <AppIcon
                          icon={ArrowRight01Icon}
                          size={14}
                          stroke={1.5}
                          className="text-muted-foreground transition-transform group-hover/event:translate-x-0.5"
                          aria-hidden
                        />
                      </div>
                    </TimelineHeader>
                    <TimelineContent className="mt-2 flex flex-wrap gap-1.5">
                      <StateBadge
                        value={
                          request.terminal_success === true
                            ? 'succeeded'
                            : request.terminal_success === false
                              ? 'failed'
                              : 'unknown'
                        }
                      />
                      <Badge variant="outline">
                        {humanize(request.correlation_confidence)} confidence
                      </Badge>
                      <Badge variant="outline">{formatRequestDuration(request)}</Badge>
                      <Badge variant="outline">
                        {requestObservations.length}{' '}
                        {requestObservations.length === 1 ? 'activity' : 'activities'}
                      </Badge>
                    </TimelineContent>
                  </Link>
                </TimelineItem>
              )
            })}
          </Timeline>
          {visibleCount < requests.length ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setVisibleCount((count) => Math.min(count + 25, requests.length))}
            >
              Show 25 more requests
            </Button>
          ) : null}
        </div>
      ) : (
        <p className="text-muted-foreground text-sm">No requests recorded.</p>
      )}
    </DiagnosticSection>
  )
}

function ToolExposure({ observations }: { observations: AgentObservationView[] }) {
  const tools = useMemo(() => summarizeToolExposure(observations), [observations])
  const used = tools.filter((tool) => tool.callCount > 0)
  const neverCalled = tools.filter((tool) => tool.callCount === 0)

  return (
    <DiagnosticSection
      title="Tool exposure"
      summary={`${used.length} used · ${neverCalled.length} never called`}
    >
      <div className="grid gap-3 xl:grid-cols-2">
        <ToolExposurePanel
          title="Used at least once"
          description="Tools called during this session."
          tools={used}
          showCallCount
        />
        <ToolExposurePanel
          title="Never called"
          description="Supplied tools with no detected call."
          tools={neverCalled}
        />
      </div>
    </DiagnosticSection>
  )
}

function ToolExposurePanel({
  title,
  description,
  tools,
  showCallCount = false,
}: {
  title: string
  description: string
  tools: ToolExposureItem[]
  showCallCount?: boolean
}) {
  const [expanded, setExpanded] = useState(false)
  const visible = expanded ? tools : tools.slice(0, TOOL_PREVIEW_LIMIT)

  return (
    <section className="overflow-hidden rounded-lg border">
      <header className="flex items-start justify-between gap-3 px-3 py-3">
        <div>
          <h4 className="font-medium">{title}</h4>
          <p className="text-muted-foreground mt-0.5 text-xs">{description}</p>
        </div>
        <Badge variant="outline" className="tabular-nums">
          {tools.length}
        </Badge>
      </header>
      {visible.length > 0 ? (
        <ul className="divide-y border-t">
          {visible.map((tool) => (
            <li key={tool.name} className="flex items-center gap-3 px-3 py-2.5">
              <div className="min-w-0 flex-1">
                <p className="truncate font-mono text-xs">{tool.name}</p>
                {showCallCount ? (
                  <p className="text-muted-foreground mt-0.5 text-xs tabular-nums">
                    {tool.callCount} {tool.callCount === 1 ? 'call' : 'calls'}
                  </p>
                ) : null}
              </div>
              <Badge variant="outline" className="shrink-0 tabular-nums">
                {tool.totalTokens.toLocaleString()} tokens
              </Badge>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-muted-foreground border-t px-3 py-4 text-sm">
          {showCallCount
            ? 'No tool calls were detected.'
            : 'No uncalled tools were present in retained tool-definition metadata.'}
        </p>
      )}
      {tools.length > TOOL_PREVIEW_LIMIT ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="w-full rounded-none border-t"
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? 'Show fewer' : `Show all ${tools.length}`}
        </Button>
      ) : null}
    </section>
  )
}

function summarizeToolExposure(observations: AgentObservationView[]): ToolExposureItem[] {
  const tools = new Map<string, ToolExposureItem & { hasDefinitionTokens: boolean }>()
  for (const observation of observations) {
    for (const supplied of observation.facts.supplied_tools ?? []) {
      const name = supplied.name.trim()
      if (!name) continue
      const tool = tools.get(name) ?? {
        name,
        totalTokens: 0,
        callCount: 0,
        hasDefinitionTokens: true,
      }
      tool.totalTokens += supplied.token_estimate
      tool.hasDefinitionTokens = true
      tools.set(name, tool)
    }
  }
  for (const observation of observations) {
    const name = observation.facts.tool_name?.trim()
    if (!name) continue
    const tool = tools.get(name) ?? {
      name,
      totalTokens: 0,
      callCount: 0,
      hasDefinitionTokens: false,
    }
    tool.callCount += 1
    if (!tool.hasDefinitionTokens) {
      tool.totalTokens += observation.facts.tool_schema_token_estimate ?? 0
    }
    tools.set(name, tool)
  }
  return [...tools.values()].sort(
    (left, right) => right.totalTokens - left.totalTokens || left.name.localeCompare(right.name),
  )
}

function formatRequestDuration(request: AgentSessionRequestView) {
  if (!request.completed_at) return 'In progress'
  return formatDuration(
    Math.max(0, new Date(request.completed_at).getTime() - new Date(request.occurred_at).getTime()),
  )
}

function formatRequestCountSummary(count: number, truncated: boolean) {
  return `${count}${truncated ? '+' : ''} ${count === 1 ? 'request' : 'requests'}`
}

function DiagnosticSection({
  title,
  summary,
  defaultOpen = false,
  children,
}: {
  title: string
  summary?: string
  defaultOpen?: boolean
  children: React.ReactNode
}) {
  return (
    <Collapsible defaultOpen={defaultOpen} className="overflow-hidden border-x">
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
        <div className="space-y-3 border-t px-4 py-4">{children}</div>
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

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 p-3">
      <p className="text-muted-foreground text-xs">{label}</p>
      <p className="mt-1 text-base leading-tight font-semibold tabular-nums sm:text-lg">{value}</p>
    </div>
  )
}

function DetailSkeleton() {
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        {Array.from({ length: 4 }).map((_, index) => (
          <Skeleton key={index} className="h-20" />
        ))}
      </div>
      <Skeleton className="h-36" />
      <Skeleton className="h-52" />
    </div>
  )
}

function StateBadge({ value }: { value: string }) {
  const normalized = value.toLowerCase()
  const variant = normalized === 'failed' ? 'destructive' : 'outline'
  return <Badge variant={variant}>{humanize(value)}</Badge>
}

function filtersFromSearch(search: AgentSessionFiltersInput): Filter<string>[] {
  return sessionFilterFields.flatMap((field) => {
    const value = search[field]
    return value === null || value === undefined || value === ''
      ? []
      : [{ id: field, field, operator: 'is', values: [String(value)] }]
  })
}

function normalizeSearch(search: Record<string, unknown>): AgentSessionRouteSearch {
  const result: AgentSessionRouteSearch = {
    page: positiveInteger(search.page, 1),
    page_size: positiveInteger(search.page_size, 50, 100),
  }
  for (const field of [
    'user_id',
    'team_id',
    'service_account_id',
    'harness_key',
    'requested_model_key',
    'operation',
    'caller_class',
    'gateway_outcome',
    'score_maturity',
    'score_confidence',
    'session_source_id',
    'external_session_id',
    'request_tag_key',
    'request_tag_value',
    'lifecycle',
    'started_after',
    'started_before',
    'session_id',
  ] as const) {
    const value = search[field]
    if (typeof value === 'string' && value.trim()) {
      result[field] = value.trim()
    }
  }
  const minimumCoveragePercent = integerInRange(search.minimum_coverage_percent, 0, 100)
  if (minimumCoveragePercent !== undefined) {
    result.minimum_coverage_percent = minimumCoveragePercent
  }
  return result
}

function positiveInteger(value: unknown, fallback: number, maximum = Number.MAX_SAFE_INTEGER) {
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? Math.min(parsed, maximum) : fallback
}

function integerInRange(value: unknown, minimum: number, maximum: number) {
  if (value === null || value === undefined || value === '') return undefined
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isInteger(parsed) && parsed >= minimum && parsed <= maximum ? parsed : undefined
}

function hasActiveSearch(search: AgentSessionFiltersInput) {
  return Boolean(
    sessionFilterFields.some((field) => search[field] !== null && search[field] !== undefined) ||
    search.started_after ||
    search.started_before,
  )
}

function formatTimestamp(value: string) {
  return timestampFormatter.format(new Date(value))
}

function formatCost(value?: number | null) {
  if (value === null || value === undefined) return '—'
  return currencyFormatters[value < 0.01 ? 'precise' : 'standard'].format(value)
}

function formatDuration(value?: number | null) {
  if (value === null || value === undefined) return '—'
  if (value < 1_000) return `${value} ms`
  if (value < 60_000) return `${(value / 1_000).toFixed(1)} s`
  return `${(value / 60_000).toFixed(1)} min`
}

function formatBasisPoints(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : `${(value / 100).toFixed(1)}%`
}

function formatPercent(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : `${value}%`
}

function formatCount(value?: number | null) {
  return value === null || value === undefined ? '—' : value.toLocaleString()
}

function formatTokenCount(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : value.toLocaleString()
}

function formatBytes(value?: number | null) {
  if (value === null || value === undefined) return 'Not available'
  if (value < 1_024) return `${value} B`
  return `${(value / 1_024).toFixed(1)} KiB`
}

function formatScaledCost(value?: number | null) {
  return value === null || value === undefined ? 'Not available' : formatCost(value / 10_000)
}

function formatNullable(value: unknown, fallback = '—') {
  return value === null || value === undefined ? fallback : String(value)
}

const limitationLabels: Record<string, string> = {
  cohort_fallback: 'The score uses a broader comparison group',
  late_data_excluded: 'Late data is not included',
  payload_truncated: 'Response data is incomplete',
  payload_unavailable: 'Response data is not available',
  pricing_unavailable: 'Pricing data is not available',
  request_incomplete: 'A request did not complete',
  semantic_verification_unavailable: 'Answer verification is not available',
  session_unobserved: 'An external session ID was not observed',
  tool_inventory_potential_only: 'The available tool list is estimated',
  usage_unavailable: 'Usage data is not available',
}

function formatLimitation(value: string) {
  return limitationLabels[value] ?? humanize(value)
}

function humanize(value?: string | null) {
  return value
    ? value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
    : 'Unknown'
}
