import { useEffect, useRef, useState, useTransition, type ReactNode } from 'react'
import { Link, createFileRoute, useRouter } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { BrandIcon } from '@/components/icons/brand-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { cn } from '@/lib/utils'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getObservabilityRequestLogDetail, getRequestLogs } from '@/server/admin-data.functions'
import type {
  RequestAttemptView,
  RequestLogDetailView,
  RequestLogFiltersInput,
  RequestLogView,
} from '@/types/api'

export const Route = createFileRoute('/observability/request-logs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeFilterSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getRequestLogs({ data: deps }),
  component: RequestLogsPage,
})

const initialFilters: RequestLogFiltersInput = {
  request_id: '',
  model_key: '',
  provider_key: '',
  service: '',
  component: '',
  env: '',
  tag_key: '',
  tag_value: '',
}

const requestLogRowEstimatePx = 56
const requestLogDesktopPreviewRows = 12
const requestLogDesktopTableHeightPx = requestLogRowEstimatePx * requestLogDesktopPreviewRows

export function RequestLogsPage() {
  const { data: logPage } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const parentRef = useRef<HTMLDivElement | null>(null)
  const [filters, setFilters] = useState<RequestLogFiltersInput>(() => ({
    ...initialFilters,
    ...search,
  }))
  const [selectedLogId, setSelectedLogId] = useState<string | null>(null)
  const [selectedDetail, setSelectedDetail] = useState<RequestLogDetailView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [isListPending, startListTransition] = useTransition()

  useEffect(() => {
    setFilters({ ...initialFilters, ...search })
  }, [search])

  useEffect(() => {
    if (!selectedLogId) {
      setSelectedDetail(null)
      setDetailPending(false)
      setDetailError(null)
      return
    }

    let cancelled = false
    setDetailPending(true)
    setDetailError(null)

    void getObservabilityRequestLogDetail({ data: { requestLogId: selectedLogId } })
      .then((response) => {
        if (!cancelled) {
          setSelectedDetail(response.data)
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDetailError(
            error instanceof Error ? error.message : 'Failed to load request log detail',
          )
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDetailPending(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [selectedLogId])

  const rowVirtualizer = useVirtualizer({
    count: logPage.items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => requestLogRowEstimatePx,
    overscan: 12,
  })

  const rows = rowVirtualizer.getVirtualItems()

  function openDetail(requestLogId: string) {
    setSelectedLogId(requestLogId)
    setSelectedDetail(null)
    setDetailPending(true)
    setDetailError(null)
  }

  function applyFilters(nextFilters: RequestLogFiltersInput) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/request-logs',
        search: normalizeFilterSearch(nextFilters),
      })
    })
  }

  function updateFilter(key: keyof RequestLogFiltersInput, value: string) {
    setFilters((current) => ({ ...current, [key]: value }))
  }

  const normalizedFilters = normalizeFilterSearch(filters)
  const hasPartialTagFilter =
    Boolean(normalizedFilters.tag_key) !== Boolean(normalizedFilters.tag_value)

  return (
    <>
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Request Logs</CardTitle>
            <CardDescription>
              Inspect single-route request execution, latency, and sanitized payloads without
              dropping into raw traces.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-5">
            <Input
              data-testid="request-log-filter-service"
              placeholder="Service"
              value={filters.service ?? ''}
              onChange={(event) => updateFilter('service', event.target.value)}
            />
            <Input
              data-testid="request-log-filter-component"
              placeholder="Component"
              value={filters.component ?? ''}
              onChange={(event) => updateFilter('component', event.target.value)}
            />
            <Input
              data-testid="request-log-filter-env"
              placeholder="Environment"
              value={filters.env ?? ''}
              onChange={(event) => updateFilter('env', event.target.value)}
            />
            <Input
              data-testid="request-log-filter-tag-key"
              placeholder="Tag key"
              value={filters.tag_key ?? ''}
              onChange={(event) => updateFilter('tag_key', event.target.value)}
            />
            <Input
              data-testid="request-log-filter-tag-value"
              placeholder="Tag value"
              value={filters.tag_value ?? ''}
              onChange={(event) => updateFilter('tag_value', event.target.value)}
            />
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="secondary"
              onClick={() => applyFilters(normalizedFilters)}
              disabled={isListPending || hasPartialTagFilter}
            >
              {isListPending ? 'Filtering...' : 'Apply Filters'}
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                setFilters(initialFilters)
                applyFilters(initialFilters)
              }}
              disabled={isListPending}
            >
              Clear
            </Button>
          </div>
          {hasPartialTagFilter ? (
            <Alert>
              <AlertTitle>Incomplete tag filter</AlertTitle>
              <AlertDescription>
                Provide both a tag key and tag value to filter bespoke request tags.
              </AlertDescription>
            </Alert>
          ) : null}
          <div className="text-sm text-[var(--color-text-soft)]">
            {logPage.total} total logs loaded from gateway observability APIs.
          </div>

          <div
            className="max-h-[34rem] overflow-auto rounded-md border border-[color:var(--color-border)] p-3 lg:hidden"
            data-testid="request-log-mobile-list"
          >
            <div className="flex flex-col gap-3">
              {logPage.items.map((item) => (
                <article
                  key={item.request_log_id}
                  className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="flex items-center gap-2 truncate font-semibold text-[var(--color-text)]">
                        <BrandIcon iconKey={item.model_icon_key} size={16} />
                        <span className="truncate">{item.model_key}</span>
                      </p>
                      <p className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                        {item.request_id}
                      </p>
                    </div>
                    <Badge variant={badgeVariant(item.status_code)}>
                      {item.status_code ?? 'n/a'}
                    </Badge>
                  </div>

                  <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Provider
                      </dt>
                      <dd className="flex items-center gap-2 text-[var(--color-text-muted)]">
                        <BrandIcon iconKey={item.provider_icon_key} size={14} />
                        <span>{item.provider_key}</span>
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Caller
                      </dt>
                      <dd className="truncate text-[var(--color-text-muted)]">
                        {callerPrimary(item) ?? 'Unknown'}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Key
                      </dt>
                      <dd className="truncate text-[var(--color-text-muted)]">
                        {item.api_key_name ?? item.api_key_id}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Latency
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">
                        {formatLatency(item.latency_ms)}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Tokens
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">
                        {formatTokenCount(item.total_tokens)}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Tools
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">
                        <ToolCardinalityInline item={item} />
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Timestamp
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">
                        {formatOccurredAt(item.occurred_at)}
                      </dd>
                    </div>
                  </dl>

                  <div className="mt-4 flex justify-end">
                    <Button
                      type="button"
                      variant="secondary"
                      onClick={() => openDetail(item.request_log_id)}
                    >
                      Inspect
                    </Button>
                  </div>
                </article>
              ))}
            </div>
          </div>

          <div
            className="hidden min-w-0 overflow-x-auto rounded-md border border-[color:var(--color-border)] lg:block"
            data-testid="request-log-desktop-table"
          >
            <div className="min-w-[80rem]">
              <div className="grid grid-cols-[minmax(13rem,1.2fr)_minmax(12rem,1.1fr)_minmax(11rem,1fr)_minmax(9rem,0.9fr)_80px_88px_80px_150px_110px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
                <span className="px-3 py-2 font-semibold">Request</span>
                <span className="px-3 py-2 font-semibold">Model</span>
                <span className="px-3 py-2 font-semibold">Caller</span>
                <span className="px-3 py-2 font-semibold">Key</span>
                <span className="px-3 py-2 font-semibold">Status</span>
                <span className="px-3 py-2 font-semibold">Latency</span>
                <span className="px-3 py-2 font-semibold">Tokens</span>
                <span className="px-3 py-2 font-semibold">Tools</span>
                <span className="px-3 py-2 font-semibold">Inspect</span>
              </div>
              <div
                ref={parentRef}
                className="overflow-y-auto"
                data-testid="request-log-desktop-table-viewport"
                style={{ height: `${requestLogDesktopTableHeightPx}px` }}
              >
                <div
                  className="relative"
                  style={{
                    height: `${rowVirtualizer.getTotalSize()}px`,
                  }}
                >
                  {rows.map((virtualRow) => {
                    const item = logPage.items[virtualRow.index]
                    return (
                      <div
                        key={item.request_log_id}
                        className="absolute top-0 left-0 grid w-full grid-cols-[minmax(13rem,1.2fr)_minmax(12rem,1.1fr)_minmax(11rem,1fr)_minmax(9rem,0.9fr)_80px_88px_80px_150px_110px] border-t border-[color:var(--color-border)] align-top text-sm"
                        style={{
                          height: `${virtualRow.size}px`,
                          transform: `translateY(${virtualRow.start}px)`,
                        }}
                      >
                        <div className="min-w-0 px-3 py-3">
                          <div className="truncate font-mono text-xs text-[var(--color-text)]">
                            {item.request_id}
                          </div>
                          <div className="truncate text-xs text-[var(--color-text-soft)]">
                            {formatOccurredAt(item.occurred_at)}
                          </div>
                        </div>
                        <div className="min-w-0 px-3 py-3">
                          <div className="flex items-center gap-2 truncate text-[var(--color-text)]">
                            <BrandIcon iconKey={item.model_icon_key} size={16} />
                            <span className="truncate">{item.model_key}</span>
                          </div>
                          <div className="mt-0.5 flex items-center gap-2 truncate text-xs text-[var(--color-text-soft)]">
                            <BrandIcon iconKey={item.provider_icon_key} size={12} />
                            <span className="truncate">{item.provider_key}</span>
                          </div>
                        </div>
                        <div className="min-w-0 px-3 py-3">
                          <div className="truncate text-[var(--color-text)]">
                            {callerPrimary(item) ?? 'Unknown'}
                          </div>
                          {callerSecondary(item) ? (
                            <div className="truncate text-xs text-[var(--color-text-soft)]">
                              {callerSecondary(item)}
                            </div>
                          ) : null}
                        </div>
                        <span className="truncate px-3 py-3 text-[var(--color-text-muted)]">
                          {item.api_key_name ?? item.api_key_id}
                        </span>
                        <span className="px-3 py-3">
                          <Badge variant={badgeVariant(item.status_code)}>
                            {item.status_code ?? 'n/a'}
                          </Badge>
                        </span>
                        <span className="px-3 py-3 text-[var(--color-text-muted)]">
                          {formatLatency(item.latency_ms)}
                        </span>
                        <span className="px-3 py-3 text-[var(--color-text-muted)]">
                          {formatTokenCount(item.total_tokens)}
                        </span>
                        <span className="px-3 py-3 text-[var(--color-text-muted)]">
                          <ToolCardinalityInline item={item} />
                        </span>
                        <div className="px-3 py-2.5">
                          <Button
                            type="button"
                            variant="secondary"
                            className="w-full"
                            onClick={() => openDetail(item.request_log_id)}
                          >
                            Inspect
                          </Button>
                        </div>
                      </div>
                    )
                  })}
                </div>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      <Sheet open={selectedLogId !== null} onOpenChange={(open) => !open && setSelectedLogId(null)}>
        <SheetContent
          side="right"
          className="gap-0 data-[side=right]:w-full data-[side=right]:sm:max-w-[min(1280px,94vw)]"
        >
          <SheetHeader className="border-b border-[color:var(--color-border)]">
            <SheetTitle>Request Log Detail</SheetTitle>
            <SheetDescription>
              Review summary fields and sanitized request and response payloads.
            </SheetDescription>
          </SheetHeader>

          <div className="flex-1 overflow-y-auto p-4">
            {detailPending ? (
              <DetailSkeleton />
            ) : detailError ? (
              <Alert variant="destructive">
                <AlertTitle>Request log detail failed</AlertTitle>
                <AlertDescription>{detailError}</AlertDescription>
              </Alert>
            ) : selectedDetail ? (
              <div className="flex flex-col gap-4">
                <div className="grid gap-3 rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4 sm:grid-cols-2 xl:grid-cols-4">
                  <DetailRow label="Request ID" value={selectedDetail.log.request_id} mono />
                  <DetailRow
                    label="Request Log ID"
                    value={selectedDetail.log.request_log_id}
                    mono
                  />
                  <DetailRow
                    label="API Key"
                    value={selectedDetail.log.api_key_name ?? selectedDetail.log.api_key_id}
                    mono={!selectedDetail.log.api_key_name}
                  />
                  <DetailRow label="Caller" value={callerLabel(selectedDetail.log)} />
                  <DetailRow
                    label="Model"
                    value={
                      <span className="inline-flex items-center gap-2">
                        <BrandIcon iconKey={selectedDetail.log.model_icon_key} size={16} />
                        <span>{selectedDetail.log.model_key}</span>
                      </span>
                    }
                  />
                  <DetailRow
                    label="Resolved Model"
                    value={
                      <span className="inline-flex items-center gap-2">
                        <BrandIcon iconKey={selectedDetail.log.model_icon_key} size={16} />
                        <span>{selectedDetail.log.resolved_model_key}</span>
                      </span>
                    }
                  />
                  <DetailRow
                    label="Provider"
                    value={
                      <span className="inline-flex items-center gap-2">
                        <BrandIcon iconKey={selectedDetail.log.provider_icon_key} size={14} />
                        <span>{selectedDetail.log.provider_key}</span>
                      </span>
                    }
                  />
                  <DetailRow label="Occurred At" value={selectedDetail.log.occurred_at} />
                  <DetailRow
                    label="Status"
                    value={
                      selectedDetail.log.status_code !== null
                        ? String(selectedDetail.log.status_code)
                        : 'n/a'
                    }
                  />
                  <DetailRow label="Latency" value={formatLatency(selectedDetail.log.latency_ms)} />
                  <DetailRow
                    label="Tokens"
                    value={formatTokenCount(selectedDetail.log.total_tokens)}
                  />
                  <OperationDetailRow item={selectedDetail.log} />
                  <DetailRow
                    label="Stream"
                    value={metadataBoolean(selectedDetail.log, 'stream') ? 'yes' : 'no'}
                  />
                  <DetailRow label="Agent Harness" value={selectedDetail.log.agent_harness_label} />
                  <DetailRow
                    label="User-Agent"
                    value={selectedDetail.user_agent_raw ?? 'n/a'}
                    mono={Boolean(selectedDetail.user_agent_raw)}
                  />
                </div>

                <div className="grid gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
                  <ToolCardinalityCard item={selectedDetail.log} />

                  <Card>
                    <CardHeader>
                      <CardTitle>Request Tags</CardTitle>
                    </CardHeader>
                    <CardContent className="flex flex-wrap gap-2">
                      <RequestTagBadges item={selectedDetail.log} />
                    </CardContent>
                  </Card>
                </div>

                <McpTokenOverheadCard detail={selectedDetail} />

                <AttemptsSection attempts={selectedDetail.attempts} />

                <PayloadSection detail={selectedDetail} />
              </div>
            ) : (
              <DetailSkeleton />
            )}
          </div>
        </SheetContent>
      </Sheet>
    </>
  )
}

function DetailRow({
  label,
  value,
  mono = false,
}: {
  label: string
  value: ReactNode
  mono?: boolean
}) {
  return (
    <div>
      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
        {label}
      </dt>
      <dd
        className={
          mono
            ? 'font-mono text-sm break-all text-[var(--color-text)]'
            : 'text-sm text-[var(--color-text)]'
        }
      >
        {value}
      </dd>
    </div>
  )
}

function DetailSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-20 w-full" />
      <Skeleton className="h-32 w-full" />
      <Skeleton className="h-48 w-full" />
    </div>
  )
}

function McpTokenOverheadCard({ detail }: { detail: RequestLogDetailView }) {
  const overhead = detail.mcp_token_overhead
  if (!overhead) {
    return null
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>MCP Token Overhead</CardTitle>
        <CardDescription>Context-window estimate, not spend accounting.</CardDescription>
      </CardHeader>
      <CardContent>
        <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <DetailRow
            label="Definition Tokens"
            value={formatTokenCount(overhead.estimated_definition_tokens)}
          />
          <DetailRow label="Tools" value={String(overhead.exposed_tool_count)} />
          <DetailRow label="Estimator" value={overhead.estimator_source} mono />
          <DetailRow label="Confidence" value={overhead.confidence} />
          <DetailRow label="Cache Hits" value={String(overhead.cache_hit_count)} />
          <DetailRow label="Cache Misses" value={String(overhead.cache_miss_count)} />
          <DetailRow
            label="Context Window"
            value={formatTokenCount(overhead.context_window_tokens)}
          />
          <DetailRow
            label="Context Share"
            value={formatBasisPoints(overhead.context_window_percent_bps)}
          />
        </dl>
      </CardContent>
    </Card>
  )
}

function ToolCardinalityInline({ item }: { item: RequestLogView }) {
  const counts = item.tool_cardinality

  return (
    <span className="inline-flex flex-wrap gap-x-2 gap-y-1 text-xs tabular-nums">
      <span>MCP {formatToolCount(counts.referenced_mcp_server_count)}</span>
      <span>exposed {formatToolCount(counts.exposed_tool_count)}</span>
      <span>called {formatToolCount(counts.invoked_tool_count)}</span>
      <span>filtered {formatToolCount(counts.filtered_tool_count)}</span>
    </span>
  )
}

function ToolCardinalityCard({ item }: { item: RequestLogView }) {
  const counts = item.tool_cardinality

  return (
    <Card>
      <CardHeader>
        <CardTitle>MCP &amp; Tools</CardTitle>
        <CardAction>
          <Button type="button" variant="outline" size="sm" asChild>
            <Link to="/observability/mcp-invocations" search={{ request_id: item.request_id }}>
              View MCP Invocations
            </Link>
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent>
        <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
          <DetailRow
            label="MCP Servers"
            value={formatToolCount(counts.referenced_mcp_server_count)}
          />
          <DetailRow label="Tools Exposed" value={formatToolCount(counts.exposed_tool_count)} />
          <DetailRow label="Tools Called" value={formatToolCount(counts.invoked_tool_count)} />
          <DetailRow label="Tools Filtered" value={formatToolCount(counts.filtered_tool_count)} />
        </dl>
      </CardContent>
    </Card>
  )
}

function AttemptsSection({ attempts }: { attempts: RequestAttemptView[] }) {
  const recordedAttempts = attempts ?? []

  return (
    <Card>
      <CardHeader>
        <CardTitle>Provider Attempts</CardTitle>
        <CardDescription>
          Ordered upstream provider execution records for this request log.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {recordedAttempts.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No provider attempts recorded</EmptyTitle>
              <EmptyDescription>
                This request log has no persisted upstream provider attempt rows.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Attempt</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Provider</TableHead>
                <TableHead>Upstream model</TableHead>
                <TableHead>Latency</TableHead>
                <TableHead>Flags</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {recordedAttempts.map((attempt) => (
                <TableRow key={attempt.request_attempt_id}>
                  <TableCell className="font-mono">#{attempt.attempt_number}</TableCell>
                  <TableCell>
                    <div className="flex flex-col gap-1">
                      <Badge variant={attemptStatusBadgeVariant(attempt.status)}>
                        {attempt.status}
                      </Badge>
                      {attempt.error_code ? (
                        <span className="text-xs text-[var(--color-text-soft)]">
                          {attempt.error_code}
                        </span>
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell className="font-mono">{attempt.provider_key}</TableCell>
                  <TableCell className="font-mono">{attempt.upstream_model}</TableCell>
                  <TableCell>{formatLatency(attempt.latency_ms)}</TableCell>
                  <TableCell>
                    <div className="flex flex-wrap gap-1">
                      {attempt.retryable ? <Badge variant="outline">retryable</Badge> : null}
                      {attempt.terminal ? <Badge variant="outline">terminal</Badge> : null}
                      {attempt.produced_final_response ? (
                        <Badge variant="outline">final response</Badge>
                      ) : null}
                      {attempt.stream ? <Badge variant="outline">stream</Badge> : null}
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
        {recordedAttempts.some((attempt) => attempt.error_detail) ? (
          <div className="mt-4 flex flex-col gap-3">
            {recordedAttempts
              .filter((attempt) => attempt.error_detail)
              .map((attempt) => (
                <div
                  key={`${attempt.request_attempt_id}-error`}
                  className="rounded-md border border-[color:var(--color-border)] p-3"
                >
                  <div className="flex flex-wrap items-center gap-2 text-sm font-semibold">
                    <span>Attempt #{attempt.attempt_number} error detail</span>
                    {attempt.error_detail_truncated ? (
                      <Badge variant="warning">truncated</Badge>
                    ) : null}
                  </div>
                  <p className="mt-2 font-mono text-xs text-[var(--color-text-muted)]">
                    {attempt.error_detail}
                  </p>
                  <p className="mt-2 font-mono text-xs text-[var(--color-text-soft)]">
                    route: {attempt.route_id}
                  </p>
                </div>
              ))}
          </div>
        ) : null}
      </CardContent>
    </Card>
  )
}

type PayloadView = 'request' | 'response' | 'split'

function PayloadSection({ detail }: { detail: RequestLogDetailView }) {
  const [view, setView] = useState<PayloadView>('split')

  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-[var(--color-text)]">Payloads</h3>
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          aria-label="Payload view"
          value={view}
          onValueChange={(value) => {
            if (value) {
              setView(value as PayloadView)
            }
          }}
        >
          <ToggleGroupItem value="request">Request</ToggleGroupItem>
          <ToggleGroupItem value="response">Response</ToggleGroupItem>
          <ToggleGroupItem value="split">Split</ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div className={cn('grid gap-4', view === 'split' && 'xl:grid-cols-2')}>
        {view !== 'response' ? (
          <PayloadCard
            title="Request Payload"
            truncated={detail.log.request_payload_truncated}
            payload={detail.payload?.request_json}
          />
        ) : null}
        {view !== 'request' ? (
          <PayloadCard
            title="Response Payload"
            truncated={detail.log.response_payload_truncated}
            payload={detail.payload?.response_json}
          />
        ) : null}
      </div>
    </section>
  )
}

function PayloadCard({
  title,
  truncated,
  payload,
}: {
  title: string
  truncated: boolean
  payload: unknown
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardAction>
          {truncated ? (
            <Badge variant="warning">truncated</Badge>
          ) : (
            <Badge variant="outline">full</Badge>
          )}
        </CardAction>
      </CardHeader>
      <CardContent>
        {payload ? (
          <pre className="font-mono text-xs leading-6 break-words whitespace-pre-wrap text-[var(--color-text-muted)]">
            {JSON.stringify(payload, null, 2)}
          </pre>
        ) : (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No payload stored</EmptyTitle>
              <EmptyDescription>
                Payload capture was disabled or summary-only for this request.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
      </CardContent>
    </Card>
  )
}

function callerPrimary(item: RequestLogView): string | null {
  return item.user_name ?? item.service_account_name ?? null
}

function callerSecondary(item: RequestLogView): string | null {
  if (item.user_name) {
    return item.user_email ?? null
  }
  return item.service_account_name ? 'service account' : null
}

function callerLabel(item: RequestLogView): string {
  const primary = callerPrimary(item)
  if (!primary) {
    return 'Unknown'
  }
  const secondary = callerSecondary(item)
  return secondary ? `${primary} · ${secondary}` : primary
}

function formatOccurredAt(occurredAt: string) {
  // RFC3339 from the gateway; trim to a compact minute-resolution display.
  return occurredAt.replace('T', ' ').slice(0, 16)
}

function badgeVariant(statusCode: number | null): 'success' | 'warning' | 'outline' {
  if (statusCode === null) {
    return 'outline'
  }

  return statusCode >= 400 ? 'warning' : 'success'
}

function attemptStatusBadgeVariant(status: string): 'success' | 'warning' | 'outline' {
  return status === 'success' ? 'success' : status.endsWith('error') ? 'warning' : 'outline'
}

function formatLatency(latencyMs: number | null) {
  return latencyMs === null ? 'n/a' : `${latencyMs}ms`
}

function formatTokenCount(totalTokens: number | null) {
  return totalTokens === null ? 'n/a' : String(totalTokens)
}

function formatBasisPoints(value: number | null) {
  return value === null ? 'n/a' : `${(value / 100).toFixed(2)}%`
}

function formatToolCount(value: number | null | undefined) {
  return value == null ? 'n/a' : String(value)
}

function OperationDetailRow({ item }: { item: RequestLogView }) {
  const label = operationLabel(item)

  if (!label) {
    return null
  }

  return <DetailRow label="Operation" value={label} />
}

function operationLabel(item: RequestLogView) {
  const operation = item.metadata.operation
  return typeof operation === 'string' && operation.trim().length > 0
    ? formatOperation(operation)
    : null
}

function formatOperation(operation: string) {
  switch (operation) {
    case 'chat_completions':
      return 'Chat Completions'
    case 'responses':
      return 'Responses'
    case 'embeddings':
      return 'Embeddings'
    default: {
      const formatted = operation
        .split(/[_\s-]+/)
        .filter((part) => part.length > 0)
        .map((part) => part[0].toUpperCase() + part.slice(1))
        .join(' ')
      return formatted.length > 0 ? formatted : operation
    }
  }
}

function metadataBoolean(item: RequestLogView, key: string) {
  return item.metadata[key] === true
}

function RequestTagBadges({ item }: { item: RequestLogView }) {
  const tags = [
    item.request_tags.service ? `service:${item.request_tags.service}` : null,
    item.request_tags.component ? `component:${item.request_tags.component}` : null,
    item.request_tags.env ? `env:${item.request_tags.env}` : null,
    ...item.request_tags.bespoke.map((tag) => `${tag.key}:${tag.value}`),
  ].filter((value): value is string => value !== null)

  if (tags.length === 0) {
    return <span className="text-xs text-[var(--color-text-soft)]">No caller tags</span>
  }

  return (
    <>
      {tags.map((tag) => (
        <Badge key={tag} variant="outline">
          {tag}
        </Badge>
      ))}
    </>
  )
}

function normalizeFilterSearch(search: Record<string, unknown>): RequestLogFiltersInput {
  return {
    request_id: searchParamValue(search.request_id),
    model_key: searchParamValue(search.model_key),
    provider_key: searchParamValue(search.provider_key),
    service: searchParamValue(search.service),
    component: searchParamValue(search.component),
    env: searchParamValue(search.env),
    tag_key: searchParamValue(search.tag_key),
    tag_value: searchParamValue(search.tag_value),
  }
}

function searchParamValue(value: unknown): string | undefined {
  if (typeof value !== 'string') {
    return undefined
  }

  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}
