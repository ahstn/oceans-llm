import { useEffect, useMemo, useRef, useState, useTransition } from 'react'
import { Link, createFileRoute, useRouter } from '@tanstack/react-router'
import {
  getCoreRowModel,
  useReactTable,
  type ColumnDef,
  type PaginationState,
  type Updater,
} from '@tanstack/react-table'

import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import { DataGrid } from '@/components/reui/data-grid/data-grid'
import { DataGridPagination } from '@/components/reui/data-grid/data-grid-pagination'
import { DataGridTable } from '@/components/reui/data-grid/data-grid-table'
import { AgentSessionDateFilter } from '@/components/reui/agent-session-date-filter'
import { Filters, type Filter, type FilterFieldConfig } from '@/components/reui/filters'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
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
    label: 'Lifecycle',
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
        header: 'Efficiency',
        cell: ({ row }) => (
          <div>
            <p className="font-medium tabular-nums">
              {showScore ? (row.original.efficiency_score ?? '—') : 'Shadow'}
            </p>
            <p className="text-muted-foreground text-xs">
              {humanize(row.original.score_confidence)} Confidence
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
            <Badge variant="outline">{row.original.limitations.length} limitations</Badge>
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
    <main className="flex min-w-0 flex-1 flex-col gap-6 p-4 sm:p-6 lg:p-8">
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
          Outcome-aware session efficiency from passively correlated model requests. Scores remain
          experimental until calibrated cohorts are available.
        </p>
      </header>

      <Card>
        <CardHeader className="gap-1 border-b">
          <CardTitle className="text-base">Session explorer</CardTitle>
          <CardDescription>
            Filter by session ownership, harness, confidence, lifecycle, or start date.
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
              <AlertTitle>Session call metrics unavailable</AlertTitle>
              <AlertDescription>
                The API omitted tool or MCP counts for analyzed sessions. Restart the gateway after
                updating it; for local demo data, run{' '}
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
        <SheetContent className="w-full overflow-y-auto sm:max-w-3xl">
          <SheetHeader>
            <SheetTitle>Agent session diagnostics</SheetTitle>
            <SheetDescription className="font-mono text-xs">{selectedSessionId}</SheetDescription>
          </SheetHeader>
          <div className="space-y-6 px-4 pb-6">
            {detailPending ? <DetailSkeleton /> : null}
            {detailError ? (
              <Alert variant="destructive">
                <AlertTitle>Session detail unavailable</AlertTitle>
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
      <section className="grid grid-cols-2 gap-3">
        <Metric
          label="Efficiency"
          value={showScore ? formatNullable(report?.score) : 'Withheld in shadow'}
        />
        <Metric label="Outcome" value={formatNullable(report?.gateway_outcome)} />
        <Metric label="Normalized cost" value={formatCost(detail.session.normalized_cost_usd)} />
        <Metric label="Active time" value={formatDuration(detail.session.active_time_ms)} />
      </section>

      {!showScore ? (
        <Alert>
          <AlertTitle>Shadow diagnostics</AlertTitle>
          <AlertDescription>
            The headline score is withheld while session boundaries, outcomes, cohorts, and coverage
            are being calibrated.
          </AlertDescription>
        </Alert>
      ) : null}

      {detail.session.limitations.length > 0 ? (
        <Alert>
          <AlertTitle>Provisional evidence</AlertTitle>
          <AlertDescription>
            {detail.session.limitations.map(humanize).join(' · ')}
          </AlertDescription>
        </Alert>
      ) : null}

      {detail.request_history_truncated || detail.observation_history_truncated ? (
        <Alert>
          <AlertTitle>History capped</AlertTitle>
          <AlertDescription>
            This response shows at most 1,000 requests and observations. Use request logs for the
            complete retained history.
          </AlertDescription>
        </Alert>
      ) : null}

      <DiagnosticSection title="Session identity">
        <DiagnosticRow label="Model" value={detail.session.requested_model_key} />
        <DiagnosticRow label="Operation" value={detail.session.operation} />
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
          label="Excluded gap time"
          value={formatDuration(components?.excluded_gap_time_ms)}
        />
        <DiagnosticRow
          label="Summed work time"
          value={formatDuration(components?.summed_work_time_ms)}
        />
        <DiagnosticRow label="Wall time" value={formatDuration(components?.wall_time_ms)} />
        <DiagnosticRow
          label="Parallel overlap saved"
          value={formatDuration(components?.overlap_savings_ms)}
        />
        <DiagnosticRow
          label="Unknown wait time"
          value={formatDuration(components?.unknown_wait_time_ms)}
        />
      </DiagnosticSection>

      <DiagnosticSection title="Confidence and cohort">
        <DiagnosticRow label="Confidence" value={detail.session.score_confidence ?? 'Unknown'} />
        <DiagnosticRow label="Maturity" value={detail.session.score_maturity ?? 'Unknown'} />
        <DiagnosticRow
          label="Cohort"
          value={formatNullable(components?.cohort_version, 'No calibrated cohort')}
        />
        <DiagnosticRow
          label="Fallback level"
          value={formatNullable(components?.cohort_fallback_level)}
        />
        <DiagnosticRow
          label="Cohort sample"
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
          label="Cohort coverage"
          value={formatPercent(telemetryCoverage?.cohort_percent)}
        />
        <DiagnosticRow
          label="Raw evidence"
          value={
            coverage
              ? [
                  coverage.request_metadata ? 'request metadata' : null,
                  coverage.response_payload ? 'response payload' : null,
                  coverage.response_payload_truncated ? 'payload truncated' : null,
                ]
                  .filter(Boolean)
                  .join(' · ') || 'Metadata unavailable'
              : 'Unavailable'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection title="Token and cache diagnostics">
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
          label="Uncached-input baseline"
          value={formatScaledCost(diagnostics?.token_and_cache.uncached_input_cost_10000)}
        />
        <DiagnosticRow
          label="Pricing policies"
          value={diagnostics?.token_and_cache.pricing_policy_versions.join(' · ') || 'Unavailable'}
        />
      </DiagnosticSection>

      <DiagnosticSection title="Tools and changes">
        <DiagnosticRow
          label="Tool calls"
          value={
            diagnostics
              ? `${diagnostics.tools_and_changes.classified_tool_calls} classified / ${diagnostics.tools_and_changes.observed_tool_calls} observed`
              : 'Unavailable'
          }
        />
        <DiagnosticRow
          label="Direct MCP calls"
          value={formatTokenCount(diagnostics?.tools_and_changes.direct_mcp_calls)}
        />
        <DiagnosticRow
          label="Supplied tool definitions"
          value={formatTokenCount(diagnostics?.tools_and_changes.supplied_tool_definitions)}
        />
        <DiagnosticRow
          label="Supplied schema bytes"
          value={formatBytes(diagnostics?.tools_and_changes.supplied_tool_schema_bytes)}
        />
        <DiagnosticRow
          label="Opaque files"
          value={formatNullable(diagnostics?.tools_and_changes.unique_opaque_files)}
        />
        <DiagnosticRow
          label="File activity"
          value={
            diagnostics
              ? `${diagnostics.tools_and_changes.file_reads_suspected} read · ${diagnostics.tools_and_changes.file_searches_suspected} search · ${diagnostics.tools_and_changes.file_edits_suspected} edit · ${diagnostics.tools_and_changes.file_creates_suspected} create · ${diagnostics.tools_and_changes.file_overwrites_suspected} overwrite`
              : 'Unavailable'
          }
        />
        <DiagnosticRow
          label="Suspected rework"
          value={formatNullable(diagnostics?.tools_and_changes.rework_spans_suspected)}
        />
        <DiagnosticRow
          label="Verification results"
          value={formatNullable(diagnostics?.tools_and_changes.verification_results_classified)}
        />
      </DiagnosticSection>

      <DiagnosticSection title="Context diagnostics">
        <DiagnosticRow
          label="Initial prompt"
          value={formatTokenCount(diagnostics?.context.initial_prompt_tokens)}
        />
        <DiagnosticRow
          label="Median prompt"
          value={formatTokenCount(diagnostics?.context.median_prompt_tokens)}
        />
        <DiagnosticRow
          label="P90 prompt"
          value={formatTokenCount(diagnostics?.context.p90_prompt_tokens)}
        />
        <DiagnosticRow
          label="Maximum prompt"
          value={formatTokenCount(diagnostics?.context.maximum_prompt_tokens)}
        />
        <DiagnosticRow
          label="Growth per turn"
          value={formatTokenCount(diagnostics?.context.prompt_growth_per_turn)}
        />
        <DiagnosticRow
          label="Growth per active minute"
          value={formatTokenCount(diagnostics?.context.prompt_growth_per_active_minute)}
        />
        <DiagnosticRow
          label="Suspected compactions"
          value={formatNullable(diagnostics?.context.suspected_compactions)}
        />
        <DiagnosticRow
          label="Suspected context resets"
          value={formatNullable(diagnostics?.context.suspected_context_resets)}
        />
        <DiagnosticRow
          label="Semantic verification"
          value={
            diagnostics
              ? diagnostics.semantic_verification_available
                ? 'Available'
                : 'Unavailable'
              : 'Unavailable'
          }
        />
      </DiagnosticSection>

      <DiagnosticSection title="Versioned formula">
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
          label="Cohort snapshot"
          value={detail.analysis?.cohort_snapshot_digest ?? '—'}
        />
        <DiagnosticRow label="Analysis ID" value={detail.analysis?.analysis_id ?? '—'} />
        <DiagnosticRow
          label="Input watermark"
          value={detail.analysis ? formatTimestamp(detail.analysis.input_watermark_at) : '—'}
        />
      </DiagnosticSection>

      <RequestHistory
        requests={detail.requests}
        historyTruncated={detail.request_history_truncated}
      />
      <ObservationHistory
        observations={detail.observations}
        historyTruncated={detail.observation_history_truncated}
      />
    </>
  )
}

function RequestHistory({
  requests,
  historyTruncated,
}: {
  requests: AgentSessionRequestView[]
  historyTruncated: boolean
}) {
  const [visibleCount, setVisibleCount] = useState(25)
  const visible = requests.slice(0, visibleCount)
  return (
    <DiagnosticSection title={`Requests (${requests.length}${historyTruncated ? '+' : ''})`}>
      {historyTruncated ? (
        <Alert>
          <AlertTitle>Request history truncated</AlertTitle>
          <AlertDescription>
            This view contains the first 1,000 retained requests. Use request logs for the remaining
            history.
          </AlertDescription>
        </Alert>
      ) : null}
      <div className="space-y-3">
        {visible.map((request) => (
          <RequestFact key={request.request_id} request={request} />
        ))}
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
    </DiagnosticSection>
  )
}

function ObservationHistory({
  observations,
  historyTruncated,
}: {
  observations: AgentObservationView[]
  historyTruncated: boolean
}) {
  const [visibleCount, setVisibleCount] = useState(25)
  const visible = observations.slice(0, visibleCount)
  return (
    <DiagnosticSection
      title={`Inferred observations (${observations.length}${historyTruncated ? '+' : ''})`}
    >
      {historyTruncated ? (
        <Alert>
          <AlertTitle>Observation history truncated</AlertTitle>
          <AlertDescription>
            This view contains the first 1,000 retained observations.
          </AlertDescription>
        </Alert>
      ) : null}
      {observations.length > 0 ? (
        <div className="space-y-3">
          {visible.map((observation) => (
            <ObservationFact key={observation.observation_id} observation={observation} />
          ))}
          {visibleCount < observations.length ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setVisibleCount((count) => Math.min(count + 25, observations.length))}
            >
              Show 25 more observations
            </Button>
          ) : null}
        </div>
      ) : (
        <p className="text-muted-foreground text-sm">No inferred observations.</p>
      )}
    </DiagnosticSection>
  )
}

function RequestFact({ request }: { request: AgentSessionRequestView }) {
  return (
    <div className="rounded-lg border p-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Link
          to="/observability/request-logs"
          search={{ request_id: request.request_id }}
          className="font-mono text-xs underline underline-offset-4"
        >
          {request.request_id}
        </Link>
        <Badge variant="outline">{request.correlation_confidence}</Badge>
        <StateBadge
          value={
            request.terminal_success === true
              ? 'succeeded'
              : request.terminal_success === false
                ? 'failed'
                : 'unknown'
          }
        />
      </div>
      <p className="text-muted-foreground mt-2">
        Request {request.ordinal + 1} · {formatTimestamp(request.occurred_at)}
      </p>
      {request.limitation_codes.length > 0 ? (
        <p className="text-muted-foreground mt-1 text-xs">
          {request.limitation_codes.map(humanize).join(' · ')}
        </p>
      ) : null}
    </div>
  )
}

function ObservationFact({ observation }: { observation: AgentObservationView }) {
  return (
    <div className="rounded-lg border p-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="font-medium">{humanize(observation.kind)}</span>
        <Badge variant="outline">{humanize(observation.evidence)}</Badge>
      </div>
      <p className="text-muted-foreground mt-2 text-xs">
        Source request {observation.source_request_id}
      </p>
      {observation.limitations.length > 0 ? (
        <p className="text-muted-foreground mt-1 text-xs">
          {observation.limitations.map(humanize).join(' · ')}
        </p>
      ) : null}
    </div>
  )
}

function DiagnosticSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3">
      <Separator />
      <h3 className="font-medium">{title}</h3>
      {children}
    </section>
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
    <div className="rounded-lg border p-3">
      <p className="text-muted-foreground text-xs">{label}</p>
      <p className="mt-1 text-lg font-semibold tabular-nums">{value}</p>
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
  return value === null || value === undefined ? 'Unavailable' : `${(value / 100).toFixed(1)}%`
}

function formatPercent(value?: number | null) {
  return value === null || value === undefined ? 'Unavailable' : `${value}%`
}

function formatCount(value?: number | null) {
  return value === null || value === undefined ? '—' : value.toLocaleString()
}

function formatTokenCount(value?: number | null) {
  return value === null || value === undefined ? 'Unavailable' : value.toLocaleString()
}

function formatBytes(value?: number | null) {
  if (value === null || value === undefined) return 'Unavailable'
  if (value < 1_024) return `${value} B`
  return `${(value / 1_024).toFixed(1)} KiB`
}

function formatScaledCost(value?: number | null) {
  return value === null || value === undefined ? 'Unavailable' : formatCost(value / 10_000)
}

function formatNullable(value: unknown, fallback = '—') {
  return value === null || value === undefined ? fallback : String(value)
}

function humanize(value?: string | null) {
  return value
    ? value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
    : 'Unknown'
}
