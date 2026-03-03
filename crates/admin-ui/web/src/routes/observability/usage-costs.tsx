import { createFileRoute } from '@tanstack/react-router'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { getUsageCosts } from '@/server/admin-data.functions'

export const Route = createFileRoute('/observability/usage-costs')({
  loader: () => getUsageCosts(),
  component: UsageCostsPage,
})

function UsageCostsPage() {
  const { data } = Route.useLoaderData()
  const max = Math.max(...data.map((point) => point.amountUsd))

  return (
    <Card>
      <CardHeader>
        <CardTitle>Usage Costs</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-3">
          {data.map((point) => (
            <div key={point.day} className="grid grid-cols-[48px_1fr_80px] items-center gap-3">
              <span className="text-xs text-neutral-500">{point.day}</span>
              <div className="h-2 rounded-full bg-neutral-800">
                <div
                  className="h-2 rounded-full bg-[var(--color-primary)]"
                  style={{ width: `${(point.amountUsd / max) * 100}%` }}
                />
              </div>
              <span className="text-right text-xs text-neutral-300">
                ${point.amountUsd.toFixed(2)}
              </span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
