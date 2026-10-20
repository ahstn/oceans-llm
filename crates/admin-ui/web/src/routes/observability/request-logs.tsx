import { useEffect, useRef, useState, useTransition, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { DatabaseLightningIcon } from '@hugeicons/core-free-icons'

import { CompactCount } from '@/components/data/compact-count'
import { AppIcon } from '@/components/icons/app-icon'
import { BrandIcon } from '@/components/icons/brand-icon'
import { canAccessPage } from '@/components/layout/admin-nav'
import { PageHeader } from '@/components/layout/page-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { EMPTY_VALUE, formatCount, formatDuration } from '@/lib/format'
import { cn } from '@/lib/utils'
import { getObservabilityRequestLogDetail, getRequestLogs } from '@/server/admin-data.functions'
import type { RequestLogDetailView, RequestLogFiltersInput, RequestLogView } from '@/types/api'

import {
  cacheHitLabel,
  callerPrimary,
  callerSecondary,
  formatOccurredAt,
  formatRequestCost,
  statusCodeBadgeVariant,
} from './-request-log-detail/format'
import { RequestLogDetail } from './-request-log-detail/request-log-detail'
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
  'grid grid-cols-[minmax(13.5rem,0.8fr)_minmax(12rem,1.3fr)_minmax(11rem,1.1fr)_76px_96px_92px_96px_160px_104px]'
// The Inspect column stays pinned to the right edge so it is never scrolled out of view.
const requestLogActionCell = 'bg-card shadow-sticky-edge-end sticky right-0 z-10 px-3'

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
                <p className="text-foreground truncate text-sm font-semibold whitespace-nowrap tabular-nums">
                  {formatOccurredAt(item.occurred_at)}
                </p>
                <p className="text-subtle-foreground flex items-center gap-2 truncate text-sm">
                  <BrandIcon iconKey={item.model_icon_key} size={16} />
                  <span className="truncate">{item.model_key}</span>
                </p>
              </div>
              <Badge variant={statusCodeBadgeVariant(item.status_code)}>
                {item.status_code ?? EMPTY_VALUE}
              </Badge>
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
              <MobileField label="Latency">{formatDuration(item.latency_ms)}</MobileField>
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
  // The sticky header shares the scroll element, so the rows start one header height down.
  // scrollMargin tells the virtualizer where they start; row offsets subtract it again below.
  const [headerHeight, setHeaderHeight] = useState(0)
  const rowVirtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => requestLogRowEstimatePx,
    overscan: 12,
    scrollMargin: headerHeight,
  })

  return (
    // One element scrolls both ways, so the header can stick to the top and Inspect to the right.
    <div
      ref={parentRef}
      className="border-border hidden min-w-0 overflow-auto rounded-md border lg:block"
      data-testid="request-log-desktop-table"
      style={{ height: `${requestLogDesktopTableHeightPx}px` }}
    >
      <div className="min-w-[68rem]">
        <div
          ref={(node) => {
            if (node) setHeaderHeight(node.offsetHeight)
          }}
          className={cn(
            requestLogGridColumns,
            'bg-surface-muted text-muted-foreground sticky top-0 z-20 text-sm whitespace-nowrap',
          )}
        >
          <span className="px-3 py-2 font-semibold">Time</span>
          <span className="px-3 py-2 font-semibold">Model</span>
          <span className="px-3 py-2 font-semibold">Caller</span>
          <span className="px-3 py-2 font-semibold">Status</span>
          <span className="px-3 py-2 text-right font-semibold">Cost</span>
          <span className="px-3 py-2 text-right font-semibold">Latency</span>
          <span className="px-3 py-2 text-right font-semibold">Tokens</span>
          <span className="px-3 py-2 text-right font-semibold">Tools (Used / Total)</span>
          <span className={cn(requestLogActionCell, 'bg-surface-muted py-2 font-semibold')}>
            Inspect
          </span>
        </div>
        <div data-testid="request-log-desktop-table-viewport">
          <div className="relative" style={{ height: `${rowVirtualizer.getTotalSize()}px` }}>
            {rowVirtualizer.getVirtualItems().map((virtualRow) => (
              <RequestLogDesktopRow
                key={items[virtualRow.index].request_log_id}
                item={items[virtualRow.index]}
                size={virtualRow.size}
                start={virtualRow.start - headerHeight}
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
  const primaryCaller = callerPrimary(item) ?? 'Unknown'
  const secondaryCaller = callerSecondary(item)

  return (
    <div
      className={cn(
        requestLogGridColumns,
        'group border-border hover:bg-surface-muted absolute top-0 left-0 w-full items-center border-t text-sm',
      )}
      style={{ height: `${size}px`, transform: `translateY(${start}px)` }}
    >
      <span
        className="text-foreground truncate px-3 whitespace-nowrap tabular-nums"
        title={item.occurred_at}
      >
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
      <div
        className="min-w-0 px-3"
        title={secondaryCaller ? `${primaryCaller} · ${secondaryCaller}` : primaryCaller}
      >
        <div className="text-foreground truncate">{primaryCaller}</div>
        {secondaryCaller ? (
          <div className="text-muted-foreground truncate text-xs">{secondaryCaller}</div>
        ) : null}
      </div>
      <span className="px-3">
        <Badge variant={statusCodeBadgeVariant(item.status_code)}>
          {item.status_code ?? EMPTY_VALUE}
        </Badge>
      </span>
      <span className="text-subtle-foreground px-3 text-right tabular-nums">
        {formatRequestCost(item.cost_usd_10000)}
      </span>
      <span className="text-subtle-foreground px-3 text-right whitespace-nowrap tabular-nums">
        {formatDuration(item.latency_ms)}
      </span>
      <span className="text-subtle-foreground px-3 text-right">
        <TokensWithCache item={item} />
      </span>
      <span className="text-subtle-foreground px-3 text-right">
        <ToolUsage item={item} />
      </span>
      <div
        className={cn(
          requestLogActionCell,
          'group-hover:bg-surface-muted flex items-center self-stretch',
        )}
      >
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
  const { session } = Route.useRouteContext()
  const canOpenInvocations = Boolean(session && canAccessPage(session, 'mcp_invocations'))

  return (
    <Sheet open={detail.isOpen} onOpenChange={(open) => !open && detail.close()}>
      <SheetContent
        side="right"
        data-testid="request-log-detail-sheet"
        className="gap-0 overflow-hidden data-[side=right]:w-full data-[side=right]:sm:max-w-[min(1280px,94vw)]"
      >
        <RequestLogDetail
          detail={detail.selectedDetail}
          pending={detail.pending}
          error={detail.error}
          canOpenInvocations={canOpenInvocations}
        />
      </SheetContent>
    </Sheet>
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

/**
 * Total tokens plus an OpenRouter-style cache indicator when the provider served cached prompt
 * tokens. The indicator leads so the digits keep the column's right edge.
 */
function TokensWithCache({ item }: { item: RequestLogView }) {
  const cachedTokens = item.cache_read_tokens ?? 0

  return (
    <span className="inline-flex items-center justify-end gap-1.5 tabular-nums">
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
      <CompactCount value={item.total_tokens} unit="tokens" />
    </span>
  )
}

function ToolUsage({ item }: { item: RequestLogView }) {
  const counts = item.tool_cardinality
  // Logs recorded before distinct tools were tracked only carry the raw call count.
  const usedTools = counts.invoked_distinct_tool_count ?? counts.invoked_tool_count

  return (
    <span className="tabular-nums" data-testid="request-log-tool-usage">
      {formatCount(usedTools)} / {formatCount(counts.request_tool_count)}
    </span>
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
