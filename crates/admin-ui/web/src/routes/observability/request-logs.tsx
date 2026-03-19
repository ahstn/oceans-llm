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

  function openDetail(requestLogId: string) {
    setSelectedLogId(requestLogId)
    setSelectedDetail(null)
    setDetailPending(true)
    setDetailError(null)
  }

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
          <div className="text-muted-foreground/80 text-sm">
            {data.total} total logs loaded from gateway observability APIs.
          </div>

          <div
            className="border-border max-h-[34rem] overflow-auto rounded-md border p-3 lg:hidden"
            data-testid="request-log-mobile-list"
          >
            <div className="flex flex-col gap-3">
              {data.items.map((item) => (
                <article
                  key={item.requestLogId}
                  className="border-border bg-muted rounded-lg border p-4"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="text-foreground truncate font-semibold">{item.modelKey}</p>
                      <p className="text-muted-foreground/80 truncate font-mono text-xs">
                        {item.requestId}
                      </p>
                    </div>
                    <Badge variant={badgeVariant(item.statusCode)}>
                      {item.statusCode ?? 'n/a'}
                    </Badge>
                  </div>

                  <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                    <div>
                      <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
                        Provider
                      </dt>
                      <dd className="text-muted-foreground">{item.providerKey}</dd>
                    </div>
                    <div>
                      <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
                        Latency
                      </dt>
                      <dd className="text-muted-foreground">{formatLatency(item.latencyMs)}</dd>
                    </div>
                    <div>
                      <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
                        Tokens
                      </dt>
                      <dd className="text-muted-foreground">
                        {formatTokenCount(item.totalTokens)}
                      </dd>
                    </div>
                    <div>
                      <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
                        Timestamp
                      </dt>
                      <dd className="text-muted-foreground">{item.occurredAt}</dd>
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
                      onClick={() => openDetail(item.requestLogId)}
                    >
                      Inspect
                    </Button>
                  </div>
                </article>
              ))}
            </div>
          </div>

          <div
            className="border-border hidden overflow-hidden rounded-md border lg:block"
            data-testid="request-log-desktop-table"
          >
            <div className="bg-muted text-muted-foreground/80 grid grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px_120px]">
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
                      className="border-border absolute top-0 left-0 grid w-full grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px_120px] border-t align-top text-sm"
                      style={{
                        height: `${virtualRow.size}px`,
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                    >
                      <div className="min-w-0 px-3 py-3">
                        <div className="text-muted-foreground/80 truncate font-mono text-xs">
                          {item.requestId}
                        </div>
                        <div className="text-muted-foreground truncate text-xs">
                          {item.requestLogId}
                        </div>
                      </div>
                      <span className="text-foreground truncate px-3 py-3">{item.modelKey}</span>
                      <span className="text-muted-foreground truncate px-3 py-3">
                        {item.providerKey}
                      </span>
                      <span className="px-3 py-3">
                        <Badge variant={badgeVariant(item.statusCode)}>
                          {item.statusCode ?? 'n/a'}
                        </Badge>
                      </span>
                      <span className="text-muted-foreground px-3 py-3">
                        {formatLatency(item.latencyMs)}
                      </span>
                      <span className="text-muted-foreground px-3 py-3">
                        {formatTokenCount(item.totalTokens)}
                      </span>
                      <div className="px-3 py-2.5">
                        <Button
                          type="button"
                          variant="secondary"
                          className="w-full"
                          onClick={() => openDetail(item.requestLogId)}
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
            <div className="text-muted-foreground/80 text-sm">Loading request log detail…</div>
          ) : detailError ? (
            <div className="rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-700">
              {detailError}
            </div>
          ) : selectedDetail ? (
            <div className="grid gap-4">
              <div className="border-border bg-muted grid gap-3 rounded-md border p-4 md:grid-cols-2">
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
            <div className="text-muted-foreground/80 text-sm">Loading request log detail…</div>
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
      <dt className="text-muted-foreground/80 text-xs font-semibold tracking-[0.08em] uppercase">
        {label}
      </dt>
      <dd className={mono ? 'text-foreground font-mono text-sm' : 'text-foreground text-sm'}>
        {value}
      </dd>
    </div>
  )
}

function PayloadCard({ title, note, payload }: { title: string; note: string; payload: unknown }) {
  return (
    <section className="border-border rounded-md border">
      <header className="border-border bg-muted border-b px-4 py-3">
        <h3 className="text-foreground font-semibold">{title}</h3>
        <p className="text-muted-foreground/80 text-sm">{note}</p>
      </header>
      <pre className="text-muted-foreground max-h-[360px] overflow-auto p-4 text-xs leading-6">
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
