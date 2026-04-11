import { useEffect, useRef, useState, useTransition, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { BrandIcon } from '@/components/icons/brand-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getObservabilityRequestLogDetail, getRequestLogs } from '@/server/admin-data.functions'
import type { RequestLogDetailView, RequestLogFiltersInput, RequestLogView } from '@/types/api'

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
            <div className="rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-800">
              Provide both a tag key and tag value to filter bespoke request tags.
            </div>
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
            className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] lg:block"
            data-testid="request-log-desktop-table"
          >
            <div className="grid grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px_120px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
              <span className="px-3 py-2 font-semibold">Request</span>
              <span className="px-3 py-2 font-semibold">Model</span>
              <span className="px-3 py-2 font-semibold">Provider</span>
              <span className="px-3 py-2 font-semibold">Status</span>
              <span className="px-3 py-2 font-semibold">Latency</span>
              <span className="px-3 py-2 font-semibold">Tokens</span>
              <span className="px-3 py-2 font-semibold">Inspect</span>
            </div>
            <div ref={parentRef} className="h-[430px] overflow-auto">
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
                      className="absolute top-0 left-0 grid w-full grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px_120px] border-t border-[color:var(--color-border)] align-top text-sm"
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
            <div className="text-sm text-[var(--color-text-soft)]">Loading request log detail…</div>
          ) : detailError ? (
            <div className="rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-700">
              {detailError}
            </div>
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

              <section className="rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4">
                <h3 className="text-sm font-semibold text-[var(--color-text)]">Request Tags</h3>
                <div className="mt-3 flex flex-wrap gap-2">
                  <RequestTagBadges item={selectedDetail.log} />
                </div>
              </section>

              <div className="grid gap-4 lg:grid-cols-2">
                <PayloadCard
                  title="Request Payload"
                  note={
                    selectedDetail.log.request_payload_truncated
                      ? 'Sanitized request payload was truncated before persistence.'
                      : 'Sanitized request payload.'
                  }
                  payload={selectedDetail.payload?.request_json}
                />
                <PayloadCard
                  title="Response Payload"
                  note={
                    selectedDetail.log.response_payload_truncated
                      ? 'Sanitized response payload was truncated before persistence.'
                      : 'Sanitized response payload.'
                  }
                  payload={selectedDetail.payload?.response_json}
                />
              </div>
            </div>
          ) : (
            <div className="text-sm text-[var(--color-text-soft)]">
              Loading request log detail…
            </div>
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

function PayloadCard({ title, note, payload }: { title: string; note: string; payload: unknown }) {
  return (
    <section className="rounded-md border border-[color:var(--color-border)]">
      <header className="border-b border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] px-4 py-3">
        <h3 className="font-semibold text-[var(--color-text)]">{title}</h3>
        <p className="text-sm text-[var(--color-text-soft)]">{note}</p>
      </header>
      <pre className="max-h-[360px] overflow-auto p-4 text-xs leading-6 text-[var(--color-text-muted)]">
        {payload ? JSON.stringify(payload, null, 2) : 'No payload stored.'}
      </pre>
    </section>
  )
}

function badgeVariant(statusCode: number | null): 'success' | 'warning' | 'outline' {
  if (statusCode === null) {
    return 'outline'
  }

  return statusCode >= 400 ? 'warning' : 'success'
}

function formatLatency(latencyMs: number | null) {
  return latencyMs === null ? 'n/a' : `${latencyMs}ms`
}

function formatTokenCount(totalTokens: number | null) {
  return totalTokens === null ? 'n/a' : String(totalTokens)
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
