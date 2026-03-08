import { startTransition, useMemo, useRef, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getRequestLog, getRequestLogs } from '@/server/admin-data.functions'
import type { RequestLogDetailView } from '@/types/api'

export const Route = createFileRoute('/observability/request-logs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getRequestLogs(),
  component: RequestLogsPage,
})

function RequestLogsPage() {
  const { data } = Route.useLoaderData()
  const parentRef = useRef<HTMLDivElement | null>(null)
  const [selectedLogId, setSelectedLogId] = useState<string | null>(null)
  const [selectedLog, setSelectedLog] = useState<RequestLogDetailView | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const rowVirtualizer = useVirtualizer({
    count: data.items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 44,
    overscan: 12,
  })

  const rows = rowVirtualizer.getVirtualItems()
  const selectedSummary = useMemo(
    () => data.items.find((item) => item.id === selectedLogId) ?? null,
    [data.items, selectedLogId],
  )

  async function openLog(requestId: string) {
    setSelectedLogId(requestId)
    setLoading(true)
    setError(null)
    startTransition(() => {
      setSelectedLog(null)
    })

    try {
      const response = await getRequestLog({ data: { requestId } })
      startTransition(() => {
        setSelectedLog(response.data)
      })
    } catch (fetchError) {
      setError(fetchError instanceof Error ? fetchError.message : 'Failed to load request log')
    } finally {
      setLoading(false)
    }
  }

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>Request Logs</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-[140px_90px_120px_70px_80px_90px_70px_90px] gap-2 border-b border-neutral-800 pb-2 text-[11px] uppercase tracking-[0.06em] text-neutral-500">
            <span>Request</span>
            <span>Model</span>
            <span>Provider</span>
            <span>Status</span>
            <span>Latency</span>
            <span>Tokens</span>
            <span>Mode</span>
            <span>Error</span>
          </div>
          <div
            ref={parentRef}
            className="mt-2 h-[420px] overflow-auto rounded-md border border-neutral-800"
          >
            <div
              className="relative"
              style={{
                height: `${rowVirtualizer.getTotalSize()}px`,
              }}
            >
              {rows.map((virtualRow) => {
                const item = data.items[virtualRow.index]
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => openLog(item.id)}
                    className="absolute top-0 left-0 grid w-full grid-cols-[140px_90px_120px_70px_80px_90px_70px_90px] gap-2 border-b border-neutral-900/80 px-3 text-left text-xs text-neutral-300 transition hover:bg-neutral-900/70"
                    style={{
                      height: `${virtualRow.size}px`,
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                  >
                    <span className="truncate py-3 font-mono text-[11px] text-neutral-500">
                      {item.id}
                    </span>
                    <span className="py-3">{item.model}</span>
                    <span className="truncate py-3">{item.provider}</span>
                    <span
                      className={`py-3 ${item.statusCode >= 400 ? 'text-amber-300' : 'text-emerald-400'}`}
                    >
                      {item.statusCode}
                    </span>
                    <span className="py-3">{item.latencyMs}ms</span>
                    <span className="py-3">{item.totalTokens ?? 'n/a'}</span>
                    <span className="py-3">{item.stream ? 'stream' : 'json'}</span>
                    <span className="truncate py-3 text-neutral-500">
                      {item.errorCode ?? 'ok'}
                    </span>
                  </button>
                )
              })}
            </div>
          </div>
        </CardContent>
      </Card>

      <Dialog open={selectedLogId !== null} onOpenChange={(open) => !open && setSelectedLogId(null)}>
        <DialogContent className="w-[min(960px,calc(100vw-32px))]">
          <DialogHeader>
            <DialogTitle>{selectedSummary?.id ?? 'Request log'}</DialogTitle>
            <DialogDescription>
              {selectedSummary
                ? `${selectedSummary.provider} -> ${selectedSummary.upstreamModel} at ${selectedSummary.timestamp}`
                : 'Request/response payload detail'}
            </DialogDescription>
          </DialogHeader>

          {loading ? (
            <div className="rounded-md border border-neutral-800 bg-neutral-950/50 p-4 text-sm text-neutral-400">
              Loading request payload…
            </div>
          ) : error ? (
            <div className="rounded-md border border-amber-700/40 bg-amber-950/30 p-4 text-sm text-amber-200">
              {error}
            </div>
          ) : selectedLog ? (
            <div className="grid gap-4">
              <div className="grid grid-cols-2 gap-3 text-xs text-neutral-400">
                <div className="rounded-md border border-neutral-800 bg-neutral-950/50 p-3">
                  <div>Request bytes: {selectedLog.requestBytes}</div>
                  <div>Truncated: {selectedLog.requestTruncated ? 'yes' : 'no'}</div>
                  <div className="truncate">SHA-256: {selectedLog.requestSha256}</div>
                </div>
                <div className="rounded-md border border-neutral-800 bg-neutral-950/50 p-3">
                  <div>Response bytes: {selectedLog.responseBytes}</div>
                  <div>Truncated: {selectedLog.responseTruncated ? 'yes' : 'no'}</div>
                  <div className="truncate">SHA-256: {selectedLog.responseSha256}</div>
                </div>
              </div>

              <div className="grid gap-4 md:grid-cols-2">
                <PayloadBlock
                  title="Request JSON"
                  value={selectedLog.requestJson}
                  truncated={selectedLog.requestTruncated}
                />
                <PayloadBlock
                  title="Response JSON"
                  value={selectedLog.responseJson}
                  truncated={selectedLog.responseTruncated}
                />
              </div>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>
    </>
  )
}

function PayloadBlock({
  title,
  value,
  truncated,
}: {
  title: string
  value: unknown
  truncated: boolean
}) {
  return (
    <div className="rounded-md border border-neutral-800 bg-neutral-950/50">
      <div className="flex items-center justify-between border-b border-neutral-800 px-4 py-2">
        <span className="text-xs font-medium uppercase tracking-[0.06em] text-neutral-400">
          {title}
        </span>
        {truncated ? (
          <span className="text-[11px] uppercase tracking-[0.06em] text-amber-300">Truncated</span>
        ) : null}
      </div>
      <pre className="max-h-[420px] overflow-auto px-4 py-3 text-[11px] leading-5 text-neutral-300">
        {JSON.stringify(value, null, 2)}
      </pre>
    </div>
  )
}
