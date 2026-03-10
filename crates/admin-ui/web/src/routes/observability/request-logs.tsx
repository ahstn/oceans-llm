import { useRef } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useVirtualizer } from '@tanstack/react-virtual'

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getRequestLogs } from '@/server/admin-data.functions'

export const Route = createFileRoute('/observability/request-logs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getRequestLogs(),
  component: RequestLogsPage,
})

export function RequestLogsPage() {
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
      <CardHeader className="gap-2">
        <CardTitle>Request Logs</CardTitle>
        <CardDescription>
          Inspect request IDs, routing, latency, and token usage without dropping into raw traces.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div
          className="max-h-[34rem] overflow-auto rounded-md border border-[color:var(--color-border)] p-3 lg:hidden"
          data-testid="request-log-mobile-list"
        >
          <div className="flex flex-col gap-3">
            {data.items.map((item) => (
              <article
                key={item.id}
                className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate font-semibold text-[var(--color-text)]">{item.model}</p>
                    <p className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                      {item.id}
                    </p>
                  </div>
                  <Badge variant={item.statusCode >= 400 ? 'warning' : 'success'}>
                    {item.statusCode}
                  </Badge>
                </div>

                <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                  <div>
                    <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                      Provider
                    </dt>
                    <dd className="text-[var(--color-text-muted)]">{item.provider}</dd>
                  </div>
                  <div>
                    <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                      Latency
                    </dt>
                    <dd className="text-[var(--color-text-muted)]">{item.latencyMs}ms</dd>
                  </div>
                  <div>
                    <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                      Tokens
                    </dt>
                    <dd className="text-[var(--color-text-muted)]">{item.tokens}</dd>
                  </div>
                  <div>
                    <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                      Timestamp
                    </dt>
                    <dd className="text-[var(--color-text-muted)]">{item.timestamp}</dd>
                  </div>
                </dl>
              </article>
            ))}
          </div>
        </div>

        <div className="hidden lg:block" data-testid="request-log-desktop-table">
          <div className="grid grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px] gap-3 border-b border-[color:var(--color-border)] pb-3 text-[11px] font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
            <span>Request</span>
            <span>Model</span>
            <span>Provider</span>
            <span>Status</span>
            <span>Latency</span>
            <span>Tokens</span>
          </div>
          <div
            ref={parentRef}
            className="mt-3 h-[430px] overflow-auto rounded-md border border-[color:var(--color-border)]"
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
                    className="absolute top-0 left-0 grid w-full grid-cols-[minmax(0,1.45fr)_minmax(0,1fr)_minmax(0,1fr)_88px_96px_88px] gap-3 border-b border-[color:var(--color-border)] px-4 text-sm text-[var(--color-text-muted)]"
                    style={{
                      height: `${virtualRow.size}px`,
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                  >
                    <span className="truncate py-3 font-mono text-xs text-[var(--color-text-soft)]">
                      {item.id}
                    </span>
                    <span className="truncate py-3 text-[var(--color-text)]">{item.model}</span>
                    <span className="truncate py-3">{item.provider}</span>
                    <span className="py-3">
                      <Badge variant={item.statusCode >= 400 ? 'warning' : 'success'}>
                        {item.statusCode}
                      </Badge>
                    </span>
                    <span className="py-3">{item.latencyMs}ms</span>
                    <span className="py-3">{item.tokens}</span>
                  </div>
                )
              })}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
