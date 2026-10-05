import { useEffect, useRef, useState, useTransition, type ReactNode } from 'react'
import { Link, createFileRoute, useRouter } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { DatabaseLightningIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { BrandIcon } from '@/components/icons/brand-icon'
import { canAccessPage } from '@/components/layout/admin-nav'
import { PageHeader } from '@/components/layout/page-header'
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
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { formatUsd10000Precise } from '@/lib/format'
import { cn } from '@/lib/utils'
import { getObservabilityRequestLogDetail, getRequestLogs } from '@/server/admin-data.functions'
import type {
  RequestAttemptView,
  RequestLogDetailView,
  RequestLogFiltersInput,
  RequestLogView,
} from '@/types/api'

import { RequestLogToolbar, type RequestLogFilterValues } from './-request-log-filters'

export const Route = createFileRoute('/observability/request-logs')({
  validateSearch: (search: Record<string, unknown>) => normalizeFilterSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getRequestLogs({ data: deps }),
  component: RequestLogsPage,
})

const requestLogRowEstimatePx = 56
const requestLogDesktopPreviewRows = 12
const requestLogDesktopTableHeightPx = requestLogRowEstimatePx * requestLogDesktopPreviewRows
const requestLogGridColumns =
  'grid grid-cols-[minmax(11rem,0.8fr)_minmax(12rem,1.3fr)_minmax(11rem,1.1fr)_76px_88px_84px_104px_160px_104px]'

export function RequestLogsPage() {
  const { data: logPage } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const detail = useRequestLogDetail()
  const [isListPending, startListTransition] = useTransition()

  // Patches merge into the latest location so a debounced search and a chip commit that land
  // close together cannot overwrite each other with stale filter values.
  function applyFilters(patch: RequestLogFilterValues, options?: { replace?: boolean }) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/request-logs',
        search: (previous) => normalizeFilterSearch({ ...previous, ...patch }),
        replace: options?.replace,
        resetScroll: false,
      })
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Request logs"
        description="Review each request, how long it took, and the data that the system stored."
      />

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Request list</CardTitle>
            <CardDescription>
              Filter requests, then select one to review more details.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <RequestLogToolbar
            filters={search}
            shownCount={logPage.items.length}
            totalCount={logPage.total}
            isPending={isListPending}
            onApply={applyFilters}
          />
          <RequestLogMobileList items={logPage.items} onInspect={detail.open} />
          <RequestLogDesktopTable items={logPage.items} onInspect={detail.open} />
        </CardContent>
      </Card>

      <RequestLogDetailSheet detail={detail} />
    </div>
  )
}

type RequestLogDetailState = ReturnType<typeof useRequestLogDetail>

function useRequestLogDetail() {
  const [selectedLogId, setSelectedLogId] = useState<string | null>(null)
  const [selectedDetail, setSelectedDetail] = useState<RequestLogDetailView | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!selectedLogId) {
      setSelectedDetail(null)
      setPending(false)
      setError(null)
      return
    }

    let cancelled = false
    setPending(true)
    setError(null)

    void getObservabilityRequestLogDetail({
      data: { requestLogId: selectedLogId },
    })
      .then((response) => {
        if (!cancelled) {
          setSelectedDetail(response.data)
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : 'Failed to load request log detail')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setPending(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [selectedLogId])

  function open(requestLogId: string) {
    setSelectedLogId(requestLogId)
    setSelectedDetail(null)
    setPending(true)
    setError(null)
  }

  return {
    isOpen: selectedLogId !== null,
    selectedDetail,
    pending,
    error,
    open,
    close: () => setSelectedLogId(null),
  }
}

function RequestLogMobileList({
  items,
  onInspect,
}: {
  items: RequestLogView[]
  onInspect: (requestLogId: string) => void
}) {
  return (
    <div
      className="border-border max-h-[34rem] overflow-auto rounded-md border p-3 lg:hidden"
      data-testid="request-log-mobile-list"
    >
      <div className="flex flex-col gap-3">
        {items.map((item) => (
          <article
            key={item.request_log_id}
            className="border-border bg-surface-muted rounded-lg border p-4"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <p className="text-foreground truncate text-sm font-semibold tabular-nums">
                  {formatOccurredAt(item.occurred_at)}
                </p>
                <p className="text-subtle-foreground flex items-center gap-2 truncate text-sm">
                  <BrandIcon iconKey={item.model_icon_key} size={16} />
                  <span className="truncate">{item.model_key}</span>
                </p>
              </div>
              <Badge variant={badgeVariant(item.status_code)}>{item.status_code ?? 'n/a'}</Badge>
            </div>

            <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
              <MobileField label="Provider">
                <span className="flex items-center gap-2">
                  <BrandIcon iconKey={item.provider_icon_key} size={14} />
                  <span>{item.provider_key}</span>
                </span>
              </MobileField>
              <MobileField label="Caller">{callerPrimary(item) ?? 'Unknown'}</MobileField>
              <MobileField label="Cost">{formatRequestCost(item.cost_usd_10000)}</MobileField>
              <MobileField label="Latency">{formatLatency(item.latency_ms)}</MobileField>
              <MobileField label="Tokens">
                <TokensWithCache item={item} />
              </MobileField>
              <MobileField label="Tools (Used / Total)">
                <ToolUsage item={item} />
              </MobileField>
            </dl>

            <div className="mt-4 flex justify-end">
              <Button
                type="button"
                variant="secondary"
                onClick={() => onInspect(item.request_log_id)}
              >
                Inspect
              </Button>
            </div>
          </article>
        ))}
      </div>
    </div>
  )
}

function RequestLogDesktopTable({
  items,
  onInspect,
}: {
  items: RequestLogView[]
  onInspect: (requestLogId: string) => void
}) {
  const parentRef = useRef<HTMLDivElement | null>(null)
  const rowVirtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => requestLogRowEstimatePx,
    overscan: 12,
  })

  return (
    <div
      className="border-border hidden min-w-0 overflow-x-auto rounded-md border lg:block"
      data-testid="request-log-desktop-table"
    >
      <div className="min-w-[68rem]">
        <div
          className={cn(requestLogGridColumns, 'bg-surface-muted text-muted-foreground text-sm')}
        >
          <span className="px-3 py-2 font-semibold">Time</span>
          <span className="px-3 py-2 font-semibold">Model</span>
          <span className="px-3 py-2 font-semibold">Caller</span>
          <span className="px-3 py-2 font-semibold">Status</span>
          <span className="px-3 py-2 font-semibold">Cost</span>
          <span className="px-3 py-2 font-semibold">Latency</span>
          <span className="px-3 py-2 font-semibold">Tokens</span>
          <span className="px-3 py-2 font-semibold">Tools (Used / Total)</span>
          <span className="px-3 py-2 font-semibold">Inspect</span>
        </div>
        <div
          ref={parentRef}
          className="overflow-y-auto"
          data-testid="request-log-desktop-table-viewport"
          style={{ height: `${requestLogDesktopTableHeightPx}px` }}
        >
          <div className="relative" style={{ height: `${rowVirtualizer.getTotalSize()}px` }}>
            {rowVirtualizer.getVirtualItems().map((virtualRow) => (
              <RequestLogDesktopRow
                key={items[virtualRow.index].request_log_id}
                item={items[virtualRow.index]}
                size={virtualRow.size}
                start={virtualRow.start}
                onInspect={onInspect}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

function RequestLogDesktopRow({
  item,
  size,
  start,
  onInspect,
}: {
  item: RequestLogView
  size: number
  start: number
  onInspect: (requestLogId: string) => void
}) {
  const secondaryCaller = callerSecondary(item)

  return (
    <div
      className={cn(
        requestLogGridColumns,
        'border-border hover:bg-surface-muted absolute top-0 left-0 w-full items-center border-t text-sm',
      )}
      style={{ height: `${size}px`, transform: `translateY(${start}px)` }}
    >
      <span className="text-foreground truncate px-3 tabular-nums" title={item.occurred_at}>
        {formatOccurredAt(item.occurred_at)}
      </span>
      <div className="min-w-0 px-3">
        <div className="text-foreground flex items-center gap-2 truncate">
          <BrandIcon iconKey={item.model_icon_key} size={16} />
          <span className="truncate">{item.model_key}</span>
        </div>
        <div className="text-muted-foreground mt-0.5 flex items-center gap-2 truncate text-xs">
          <BrandIcon iconKey={item.provider_icon_key} size={12} />
          <span className="truncate">{item.provider_key}</span>
        </div>
      </div>
      <div className="min-w-0 px-3">
        <div className="text-foreground truncate">{callerPrimary(item) ?? 'Unknown'}</div>
        {secondaryCaller ? (
          <div className="text-muted-foreground truncate text-xs">{secondaryCaller}</div>
        ) : null}
      </div>
      <span className="px-3">
        <Badge variant={badgeVariant(item.status_code)}>{item.status_code ?? 'n/a'}</Badge>
      </span>
      <span className="text-subtle-foreground px-3 tabular-nums">
        {formatRequestCost(item.cost_usd_10000)}
      </span>
      <span className="text-subtle-foreground px-3 tabular-nums">
        {formatLatency(item.latency_ms)}
      </span>
      <span className="text-subtle-foreground px-3">
        <TokensWithCache item={item} />
      </span>
      <span className="text-subtle-foreground px-3">
        <ToolUsage item={item} />
      </span>
      <div className="px-3">
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="w-full"
          onClick={() => onInspect(item.request_log_id)}
        >
          Inspect
        </Button>
      </div>
    </div>
  )
}

function RequestLogDetailSheet({ detail }: { detail: RequestLogDetailState }) {
  return (
    <Sheet open={detail.isOpen} onOpenChange={(open) => !open && detail.close()}>
      <SheetContent
        side="right"
        className="gap-0 data-[side=right]:w-full data-[side=right]:sm:max-w-[min(1280px,94vw)]"
      >
        <SheetHeader className="border-border border-b">
          <SheetTitle>Request Log Detail</SheetTitle>
          <SheetDescription>
            Review summary fields and sanitized request and response payloads.
          </SheetDescription>
        </SheetHeader>

        <div className="flex-1 overflow-y-auto p-4">
          {detail.error && !detail.pending ? (
            <Alert variant="destructive">
              <AlertTitle>Request log detail failed</AlertTitle>
              <AlertDescription>{detail.error}</AlertDescription>
            </Alert>
          ) : detail.selectedDetail && !detail.pending ? (
            <RequestLogDetailBody detail={detail.selectedDetail} />
          ) : (
            <DetailSkeleton />
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}

function RequestLogDetailBody({ detail }: { detail: RequestLogDetailView }) {
  const log = detail.log

  return (
    <div className="flex flex-col gap-4">
      <div className="border-border bg-surface-muted grid gap-3 rounded-md border p-4 sm:grid-cols-2 xl:grid-cols-4">
        <DetailRow label="Request ID" value={log.request_id} mono />
        <DetailRow label="Request Log ID" value={log.request_log_id} mono />
        <DetailRow
          label="API Key"
          value={log.api_key_name ?? log.api_key_id}
          mono={!log.api_key_name}
        />
        <DetailRow label="Caller" value={callerLabel(log)} />
        <DetailRow
          label="Model"
          value={
            <span className="inline-flex items-center gap-2">
              <BrandIcon iconKey={log.model_icon_key} size={16} />
              <span>{log.model_key}</span>
            </span>
          }
        />
        <DetailRow
          label="Resolved Model"
          value={
            <span className="inline-flex items-center gap-2">
              <BrandIcon iconKey={log.model_icon_key} size={16} />
              <span>{log.resolved_model_key}</span>
            </span>
          }
        />
        <DetailRow
          label="Provider"
          value={
            <span className="inline-flex items-center gap-2">
              <BrandIcon iconKey={log.provider_icon_key} size={14} />
              <span>{log.provider_key}</span>
            </span>
          }
        />
        <DetailRow label="Occurred At" value={log.occurred_at} />
        <DetailRow
          label="Status"
          value={log.status_code !== null ? String(log.status_code) : 'n/a'}
        />
        <DetailRow label="Latency" value={formatLatency(log.latency_ms)} />
        <DetailRow label="Tokens" value={formatTokenCount(log.total_tokens)} />
        <DetailRow
          label="Cached Tokens"
          value={
            log.cache_read_tokens ? cacheHitLabel(log.cache_read_tokens, log.prompt_tokens) : 'none'
          }
        />
        <DetailRow label="Cost" value={formatRequestCost(log.cost_usd_10000)} />
        <OperationDetailRow item={log} />
        <DetailRow label="Stream" value={metadataBoolean(log, 'stream') ? 'yes' : 'no'} />
        <DetailRow label="Agent Harness" value={log.agent_harness_label} />
        <DetailRow
          label="User-Agent"
          value={detail.user_agent_raw ?? 'n/a'}
          mono={Boolean(detail.user_agent_raw)}
        />
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <ToolCardinalityCard item={log} />

        <Card>
          <CardHeader>
            <CardTitle>Request Tags</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            <RequestTagBadges item={log} />
          </CardContent>
        </Card>
      </div>

      <McpTokenOverheadCard detail={detail} />

      <AttemptsSection attempts={detail.attempts} />

      <PayloadSection detail={detail} />
    </div>
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
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd
        className={mono ? 'text-foreground font-mono text-sm break-all' : 'text-foreground text-sm'}
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

function MobileField({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className="text-subtle-foreground truncate">{children}</dd>
    </div>
  )
}

/** Total tokens plus an OpenRouter-style cache indicator when the provider served cached prompt tokens. */
function TokensWithCache({ item }: { item: RequestLogView }) {
  const cachedTokens = item.cache_read_tokens ?? 0

  return (
    <span className="inline-flex items-center gap-1.5 tabular-nums">
      {formatTokenCount(item.total_tokens)}
      {cachedTokens > 0 ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="text-muted-foreground hover:text-foreground focus-visible:ring-ring/50 inline-flex rounded-sm outline-none focus-visible:ring-3"
              aria-label="Cache hit"
              data-testid="request-log-cache-hit"
            >
              <AppIcon icon={DatabaseLightningIcon} size={14} stroke={1.5} aria-hidden />
            </button>
          </TooltipTrigger>
          <TooltipContent>{cacheHitLabel(cachedTokens, item.prompt_tokens)}</TooltipContent>
        </Tooltip>
      ) : null}
    </span>
  )
}

function ToolUsage({ item }: { item: RequestLogView }) {
  const counts = item.tool_cardinality

  return (
    <span className="tabular-nums" data-testid="request-log-tool-usage">
      {formatToolCount(counts.invoked_tool_count)} / {formatToolCount(counts.request_tool_count)}
    </span>
  )
}

function ToolCardinalityCard({ item }: { item: RequestLogView }) {
  const counts = item.tool_cardinality
  const { session } = Route.useRouteContext()
  const canOpenInvocations = session && canAccessPage(session, 'mcp_invocations')

  return (
    <Card>
      <CardHeader>
        <CardTitle>MCP &amp; Tools</CardTitle>
        {canOpenInvocations ? (
          <CardAction>
            <Button type="button" variant="outline" size="sm" asChild>
              <Link to="/observability/mcp-invocations" search={{ request_id: item.request_id }}>
                View MCP Invocations
              </Link>
            </Button>
          </CardAction>
        ) : null}
      </CardHeader>
      <CardContent>
        <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-5">
          <DetailRow
            label="MCP Servers"
            value={formatToolCount(counts.referenced_mcp_server_count)}
          />
          <DetailRow label="Request Tools" value={formatToolCount(counts.request_tool_count)} />
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
                        <span className="text-muted-foreground text-xs">{attempt.error_code}</span>
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
                  className="border-border rounded-md border p-3"
                >
                  <div className="flex flex-wrap items-center gap-2 text-sm font-semibold">
                    <span>Attempt #{attempt.attempt_number} error detail</span>
                    {attempt.error_detail_truncated ? (
                      <Badge variant="warning">truncated</Badge>
                    ) : null}
                  </div>
                  <p className="text-subtle-foreground mt-2 font-mono text-xs">
                    {attempt.error_detail}
                  </p>
                  <p className="text-muted-foreground mt-2 font-mono text-xs">
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
        <h3 className="text-foreground text-sm font-semibold">Payloads</h3>
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
          <pre className="text-subtle-foreground font-mono text-xs leading-6 break-words whitespace-pre-wrap">
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
  // RFC3339 from the gateway; trim to a compact second-resolution display.
  return occurredAt.replace('T', ' ').slice(0, 19)
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

function formatRequestCost(costUsd10000: number | null | undefined) {
  return costUsd10000 == null ? '—' : formatUsd10000Precise(costUsd10000)
}

function cacheHitLabel(cachedTokens: number, promptTokens: number | null | undefined) {
  const cached = cachedTokens.toLocaleString('en-US')
  if (!promptTokens) {
    return `${cached} prompt tokens served from cache`
  }
  const percent = Math.round((cachedTokens / promptTokens) * 100)
  return `${cached} of ${promptTokens.toLocaleString('en-US')} prompt tokens cached (${percent}%)`
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
    case 'decisions':
      return 'Decisions'
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
    return <span className="text-muted-foreground text-xs">No caller tags</span>
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
    q: searchParamValue(search.q),
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
