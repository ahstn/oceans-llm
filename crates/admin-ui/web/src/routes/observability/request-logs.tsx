import { useRef } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getRequestLogs } from '@/server/admin-data.functions'

export const Route = createFileRoute('/observability/request-logs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getRequestLogs(),
  component: RequestLogsPage,
})

function RequestLogsPage() {
  const { data } = Route.useLoaderData()
  const parentRef = useRef<HTMLDivElement | null>(null)

  const rowVirtualizer = useVirtualizer({
    count: data.items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 36,
    overscan: 12,
  })

  const rows = rowVirtualizer.getVirtualItems()

  return (
    <Card>
      <CardHeader>
        <CardTitle>Request Logs</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-[120px_90px_90px_80px_90px_80px] gap-2 border-b border-neutral-800 pb-2 text-[11px] tracking-[0.06em] text-neutral-500 uppercase">
          <span>Request</span>
          <span>Model</span>
          <span>Provider</span>
          <span>Status</span>
          <span>Latency</span>
          <span>Tokens</span>
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
                <div
                  key={item.id}
                  className="absolute top-0 left-0 grid w-full grid-cols-[120px_90px_90px_80px_90px_80px] gap-2 border-b border-neutral-900/80 px-3 text-xs text-neutral-300"
                  style={{
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  <span className="truncate py-2 font-mono text-[11px] text-neutral-500">
                    {item.id}
                  </span>
                  <span className="py-2">{item.model}</span>
                  <span className="py-2">{item.provider}</span>
                  <span
                    className={`py-2 ${item.statusCode >= 400 ? 'text-amber-300' : 'text-emerald-400'}`}
                  >
                    {item.statusCode}
                  </span>
                  <span className="py-2">{item.latencyMs}ms</span>
                  <span className="py-2">{item.tokens}</span>
                </div>
              )
            })}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
