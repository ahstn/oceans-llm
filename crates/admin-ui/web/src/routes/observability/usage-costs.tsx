import { createFileRoute } from '@tanstack/react-router'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getUsageCosts } from '@/server/admin-data.functions'

export const Route = createFileRoute('/observability/usage-costs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getUsageCosts(),
  component: UsageCostsPage,
})

function UsageCostsPage() {
  const { data } = Route.useLoaderData()
  const max = Math.max(...data.map((point) => point.amountUsd))

  return (
    <Card>
      <CardHeader className="gap-2">
        <CardTitle>Usage Costs</CardTitle>
        <CardDescription>
          Weekly spend by day, kept intentionally lightweight for quick operator scans.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-4">
          {data.map((point) => (
            <div key={point.day} className="grid grid-cols-[56px_1fr_88px] items-center gap-4">
              <span className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                {point.day}
              </span>
              <div className="h-3 rounded-full bg-[color:var(--color-surface-muted)]">
                <div
                  className="h-3 rounded-full bg-[var(--color-primary)]"
                  style={{ width: `${(point.amountUsd / max) * 100}%` }}
                />
              </div>
              <span className="text-right text-sm font-semibold text-[var(--color-text)]">
                ${point.amountUsd.toFixed(2)}
              </span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
