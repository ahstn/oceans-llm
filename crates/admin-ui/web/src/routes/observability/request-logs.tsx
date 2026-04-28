import { useEffect, useRef, useState, useTransition, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { BrandIcon } from '@/components/icons/brand-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
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
    estimateSize: () => 56,
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
                      <dd className="text-[var(--color-text-muted)]">{item.occurred_at}</dd>
                    </div>
                  </dl>

                  <div className="mt-4 flex items-center justify-between gap-3">
                    <div className="flex flex-wrap gap-2">
                      {metadataBoolean(item, 'stream') ? (
                        <Badge variant="outline">stream</Badge>
                      ) : null}
                      <PayloadPolicyBadges item={item} />
                      <RequestTagBadges item={item} />
                    </div>
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
            <div className="min-w-[78rem]">
              <div className="grid grid-cols-[minmax(18rem,1.45fr)_minmax(12rem,1fr)_minmax(12rem,1fr)_88px_96px_88px_160px_120px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
                <span className="px-3 py-2 font-semibold">Request</span>
                <span className="px-3 py-2 font-semibold">Model</span>
                <span className="px-3 py-2 font-semibold">Provider</span>
                <span className="px-3 py-2 font-semibold">Status</span>
                <span className="px-3 py-2 font-semibold">Latency</span>
                <span className="px-3 py-2 font-semibold">Tokens</span>
                <span className="px-3 py-2 font-semibold">Tools</span>
                <span className="px-3 py-2 font-semibold">Inspect</span>
              </div>
              <div ref={parentRef} className="h-[430px] overflow-y-auto">
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
                        className="absolute top-0 left-0 grid w-full grid-cols-[minmax(18rem,1.45fr)_minmax(12rem,1fr)_minmax(12rem,1fr)_88px_96px_88px_160px_120px] border-t border-[color:var(--color-border)] align-top text-sm"
                        style={{
                          height: `${virtualRow.size}px`,
                          transform: `translateY(${virtualRow.start}px)`,
                        }}
                      >
                        <div className="min-w-0 px-3 py-3">
                          <div className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                            {item.request_id}
                          </div>
                          <div className="truncate text-xs text-[var(--color-text-muted)]">
                            {item.request_log_id}
                          </div>
                          <div className="mt-1 flex flex-wrap gap-1">
                            <PayloadPolicyBadges item={item} />
                          </div>
                        </div>
                        <span className="flex items-center gap-2 truncate px-3 py-3 text-[var(--color-text)]">
                          <BrandIcon iconKey={item.model_icon_key} size={16} />
                          <span className="truncate">{item.model_key}</span>
                        </span>
                        <span className="flex items-center gap-2 truncate px-3 py-3 text-[var(--color-text-muted)]">
                          <BrandIcon iconKey={item.provider_icon_key} size={14} />
                          <span className="truncate">{item.provider_key}</span>
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

      <Dialog
        open={selectedLogId !== null}
        onOpenChange={(open) => !open && setSelectedLogId(null)}
      >
        <DialogContent className="w-[min(960px,calc(100vw-32px))]">
          <DialogHeader>
            <DialogTitle>Request Log Detail</DialogTitle>
            <DialogDescription>
              Review summary fields and sanitized request and response payloads.
            </DialogDescription>
          </DialogHeader>

          {detailPending ? (
            <DetailSkeleton />
          ) : detailError ? (
            <Alert variant="destructive">
              <AlertTitle>Request log detail failed</AlertTitle>
              <AlertDescription>{detailError}</AlertDescription>
            </Alert>
          ) : selectedDetail ? (
            <div className="grid gap-4">
              <div className="grid gap-3 rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4 md:grid-cols-2">
                <DetailRow label="Request ID" value={selectedDetail.log.request_id} mono />
                <DetailRow label="Request Log ID" value={selectedDetail.log.request_log_id} mono />
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
                <DetailRow
                  label="Stream"
                  value={metadataBoolean(selectedDetail.log, 'stream') ? 'yes' : 'no'}
                />
              </div>

              <ToolCardinalityCard item={selectedDetail.log} />

              <PayloadPolicyCard log={selectedDetail.log} />

              <Card>
                <CardHeader>
                  <CardTitle>Request Tags</CardTitle>
                </CardHeader>
                <CardContent className="flex flex-wrap gap-2">
                  <RequestTagBadges item={selectedDetail.log} />
                </CardContent>
              </Card>

              <AttemptsSection attempts={selectedDetail.attempts} />

              <div className="grid gap-4 lg:grid-cols-2">
                <PayloadCard
                  title="Request Payload"
                  truncated={selectedDetail.log.request_payload_truncated}
                  payload={selectedDetail.payload?.request_json}
                />
                <PayloadCard
                  title="Response Payload"
                  truncated={selectedDetail.log.response_payload_truncated}
                  payload={selectedDetail.payload?.response_json}
                />
              </div>
            </div>
          ) : (
            <DetailSkeleton />
          )}
        </DialogContent>
      </Dialog>
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
          mono ? 'font-mono text-sm text-[var(--color-text)]' : 'text-sm text-[var(--color-text)]'
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

function PayloadPolicyCard({ log }: { log: RequestLogView }) {
  const policy = log.payload_policy
  const hasTruncation = log.request_payload_truncated || log.response_payload_truncated

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex flex-col gap-1">
            <CardTitle>Payload Policy</CardTitle>
            <CardDescription>{payloadPolicyDescription(log)}</CardDescription>
          </div>
          <PayloadPolicyBadges item={log} />
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {hasTruncation || !log.has_payload ? (
          <Alert>
            <AlertTitle>Payload capture state</AlertTitle>
            <AlertDescription>
              {hasTruncation
                ? 'One or more sanitized payloads were truncated before persistence.'
                : `Capture mode is ${formatCaptureMode(policy.capture_mode)}, so no payload body is stored for this row.`}
            </AlertDescription>
          </Alert>
        ) : null}
        <dl className="grid gap-3 text-sm md:grid-cols-4">
          <DetailRow label="Capture" value={formatCaptureMode(policy.capture_mode)} />
          <DetailRow label="Request Limit" value={formatBytes(policy.request_max_bytes)} />
          <DetailRow label="Response Limit" value={formatBytes(policy.response_max_bytes)} />
          <DetailRow label="Stream Events" value={String(policy.stream_max_events)} />
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
      </CardHeader>
      <CardContent>
        <dl className="grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-4">
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
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex flex-col gap-1">
            <CardTitle>{title}</CardTitle>
            <CardDescription>
              {truncated
                ? 'Sanitized payload was truncated before persistence.'
                : 'Sanitized payload.'}
            </CardDescription>
          </div>
          {truncated ? (
            <Badge variant="warning">truncated</Badge>
          ) : (
            <Badge variant="outline">full</Badge>
          )}
        </div>
      </CardHeader>
      <CardContent>
        {payload ? (
          <pre className="max-h-[360px] overflow-auto text-xs leading-6 text-[var(--color-text-muted)]">
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

function PayloadPolicyBadges({ item }: { item: RequestLogView }) {
  const policy = item.payload_policy
  const hasTruncation = item.request_payload_truncated || item.response_payload_truncated

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex flex-wrap gap-1">
            <Badge variant={item.has_payload ? 'secondary' : 'outline'}>
              {formatCaptureMode(policy.capture_mode)}
            </Badge>
            {item.request_payload_truncated ? <Badge variant="warning">req truncated</Badge> : null}
            {item.response_payload_truncated ? (
              <Badge variant="warning">resp truncated</Badge>
            ) : null}
            {hasTruncation ? null : item.has_payload ? (
              <Badge variant="outline">payload</Badge>
            ) : null}
          </span>
        </TooltipTrigger>
        <TooltipContent>
          {`${formatBytes(policy.request_max_bytes)} request limit, ${formatBytes(policy.response_max_bytes)} response limit, ${policy.stream_max_events} stream events, ${policy.version}`}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
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

function formatToolCount(value: number | null) {
  return value === null ? 'n/a' : String(value)
}

function formatCaptureMode(captureMode: string) {
  switch (captureMode) {
    case 'disabled':
      return 'disabled'
    case 'summary_only':
      return 'summary only'
    case 'redacted_payloads':
      return 'redacted payloads'
    default:
      return captureMode
  }
}

function formatBytes(bytes: number) {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`
  }
  if (bytes >= 1024) {
    return `${Math.round(bytes / 1024)} KiB`
  }
  return `${bytes} B`
}

function payloadPolicyDescription(log: RequestLogView) {
  const policy = log.payload_policy
  return `${formatCaptureMode(policy.capture_mode)} capture with ${formatBytes(
    policy.request_max_bytes,
  )} request and ${formatBytes(policy.response_max_bytes)} response budgets.`
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
