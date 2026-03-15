import { useEffect, useRef, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getObservabilityRequestLogDetail, getRequestLogs } from '@/server/admin-data.functions'
import type { RequestLogDetailView, RequestLogView } from '@/types/api'

export const Route = createFileRoute('/observability/request-logs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getRequestLogs(),
  component: RequestLogsPage,
})

export function RequestLogsPage() {
  const { data } = Route.useLoaderData()
  const parentRef = useRef<HTMLDivElement | null>(null)
  const [selectedLogId, setSelectedLogId] = useState<string | null>(null)
  const [selectedDetail, setSelectedDetail] = useState<RequestLogDetailView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)

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
    count: data.items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 56,
    overscan: 12,
  })

  const rows = rowVirtualizer.getVirtualItems()

  return (
    <>
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Request Logs</CardTitle>
            <CardDescription>
              Inspect routed requests, fallback behavior, latency, and sanitized payloads without
              dropping into raw traces.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="text-sm text-[var(--color-text-soft)]">
            {data.total} total logs loaded from gateway observability APIs.
          </div>

          <div
            className="max-h-[34rem] overflow-auto rounded-md border border-[color:var(--color-border)] p-3 lg:hidden"
            data-testid="request-log-mobile-list"
          >
            <div className="flex flex-col gap-3">
              {data.items.map((item) => (
                <article
                  key={item.requestLogId}
                  className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="truncate font-semibold text-[var(--color-text)]">
                        {item.modelKey}
                      </p>
                      <p className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                        {item.requestId}
                      </p>
                    </div>
                    <Badge variant={badgeVariant(item.statusCode)}>
                      {item.statusCode ?? 'n/a'}
                    </Badge>
                  </div>

                  <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Provider
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">{item.providerKey}</dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Latency
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">
                        {formatLatency(item.latencyMs)}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Tokens
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">
                        {formatTokenCount(item.totalTokens)}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                        Timestamp
                      </dt>
                      <dd className="text-[var(--color-text-muted)]">{item.occurredAt}</dd>
                    </div>
                  </dl>

                  <div className="mt-4 flex items-center justify-between gap-3">
                    <div className="flex flex-wrap gap-2">
                      {metadataBoolean(item, 'stream') ? (
                        <Badge variant="outline">stream</Badge>
                      ) : null}
                      {metadataBoolean(item, 'fallback_used') ? (
                        <Badge variant="warning">fallback</Badge>
                      ) : null}
                    </div>
                    <Button
                      type="button"
                      variant="secondary"
                      onClick={() => setSelectedLogId(item.requestLogId)}
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
                  const item = data.items[virtualRow.index]
                  return (
                    <div
                      key={item.requestLogId}
                      className="absolute top-0 left-0 grid w-full grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px_120px] border-t border-[color:var(--color-border)] align-top text-sm"
                      style={{
                        height: `${virtualRow.size}px`,
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                    >
                      <div className="min-w-0 px-3 py-3">
                        <div className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                          {item.requestId}
                        </div>
                        <div className="truncate text-xs text-[var(--color-text-muted)]">
                          {item.requestLogId}
                        </div>
                      </div>
                      <span className="truncate px-3 py-3 text-[var(--color-text)]">
                        {item.modelKey}
                      </span>
                      <span className="truncate px-3 py-3 text-[var(--color-text-muted)]">
                        {item.providerKey}
                      </span>
                      <span className="px-3 py-3">
                        <Badge variant={badgeVariant(item.statusCode)}>
                          {item.statusCode ?? 'n/a'}
                        </Badge>
                      </span>
                      <span className="px-3 py-3 text-[var(--color-text-muted)]">
                        {formatLatency(item.latencyMs)}
                      </span>
                      <span className="px-3 py-3 text-[var(--color-text-muted)]">
                        {formatTokenCount(item.totalTokens)}
                      </span>
                      <div className="px-3 py-2.5">
                        <Button
                          type="button"
                          variant="secondary"
                          className="w-full"
                          onClick={() => setSelectedLogId(item.requestLogId)}
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
              Review summary fields, fallback metadata, and sanitized request and response payloads.
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
                <DetailRow label="Request ID" value={selectedDetail.log.requestId} mono />
                <DetailRow label="Request Log ID" value={selectedDetail.log.requestLogId} mono />
                <DetailRow label="Model" value={selectedDetail.log.modelKey} />
                <DetailRow label="Resolved Model" value={selectedDetail.log.resolvedModelKey} />
                <DetailRow label="Provider" value={selectedDetail.log.providerKey} />
                <DetailRow label="Occurred At" value={selectedDetail.log.occurredAt} />
                <DetailRow
                  label="Status"
                  value={
                    selectedDetail.log.statusCode !== null
                      ? String(selectedDetail.log.statusCode)
                      : 'n/a'
                  }
                />
                <DetailRow label="Latency" value={formatLatency(selectedDetail.log.latencyMs)} />
                <DetailRow
                  label="Tokens"
                  value={formatTokenCount(selectedDetail.log.totalTokens)}
                />
                <DetailRow
                  label="Attempt Count"
                  value={String(metadataNumber(selectedDetail.log, 'attempt_count') ?? 1)}
                />
                <DetailRow
                  label="Fallback"
                  value={metadataBoolean(selectedDetail.log, 'fallback_used') ? 'yes' : 'no'}
                />
                <DetailRow
                  label="Stream"
                  value={metadataBoolean(selectedDetail.log, 'stream') ? 'yes' : 'no'}
                />
              </div>

              <div className="grid gap-4 lg:grid-cols-2">
                <PayloadCard
                  title="Request Payload"
                  note={
                    selectedDetail.log.requestPayloadTruncated
                      ? 'Sanitized request payload was truncated before persistence.'
                      : 'Sanitized request payload.'
                  }
                  payload={selectedDetail.payload?.requestJson}
                />
                <PayloadCard
                  title="Response Payload"
                  note={
                    selectedDetail.log.responsePayloadTruncated
                      ? 'Sanitized response payload was truncated before persistence.'
                      : 'Sanitized response payload.'
                  }
                  payload={selectedDetail.payload?.responseJson}
                />
              </div>
            </div>
          ) : (
            <div className="text-sm text-[var(--color-text-soft)]">Request log not found.</div>
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
  value: string
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

function metadataNumber(item: RequestLogView, key: string) {
  const value = item.metadata[key]
  return typeof value === 'number' ? value : null
}
