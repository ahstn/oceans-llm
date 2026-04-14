import { useMemo, useState, useTransition } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { requireAdminSession } from '@/routes/-admin-guard'
import { getSpendUsageReport, getUsageCosts } from '@/server/admin-data.functions'
import type { SpendOwnerKind, SpendReportView } from '@/types/api'

export const Route = createFileRoute('/observability/usage-costs')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getUsageCosts(),
  component: UsageCostsPage,
})

const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

export function UsageCostsPage() {
  const loaderData = Route.useLoaderData()
  const [report, setReport] = useState<SpendReportView>(loaderData.data)
  const [windowDays, setWindowDays] = useState<7 | 30>((loaderData.data.window_days as 7 | 30) ?? 7)
  const [ownerKind, setOwnerKind] = useState<SpendOwnerKind>(loaderData.data.owner_kind ?? 'all')
  const [isPending, startTransition] = useTransition()

  const maxDaily = useMemo(() => {
    const values = report.daily.map((point) => point.priced_cost_usd_10000 / 10_000)
    return Math.max(...values, 1)
  }, [report.daily])

  function refreshReport() {
    startTransition(async () => {
      try {
        const response = await getSpendUsageReport({
          data: {
            days: windowDays,
            owner_kind: ownerKind,
          },
        })
        setReport(response.data)
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Usage Costs</CardTitle>
            <CardDescription>
              Live spend from the durable usage ledger with owner and model breakdowns.
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Select
              value={String(windowDays)}
              onValueChange={(value) => setWindowDays(Number(value) as 7 | 30)}
            >
              <SelectTrigger className="w-[130px]">
                <SelectValue placeholder="Window" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="7">Last 7 days</SelectItem>
                  <SelectItem value="30">Last 30 days</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
            <Select
              value={ownerKind}
              onValueChange={(value) => setOwnerKind(value as SpendOwnerKind)}
            >
              <SelectTrigger className="w-[130px]">
                <SelectValue placeholder="Owner" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="all">All owners</SelectItem>
                  <SelectItem value="user">Users only</SelectItem>
                  <SelectItem value="team">Teams only</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
            <Button type="button" variant="secondary" onClick={refreshReport} disabled={isPending}>
              {isPending ? 'Refreshing...' : 'Refresh'}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-3 md:grid-cols-4">
            <MetricCard
              label="Priced spend"
              value={CURRENCY_FORMATTER.format(report.totals.priced_cost_usd_10000 / 10_000)}
            />
            <MetricCard
              label="Priced requests"
              value={String(report.totals.priced_request_count)}
            />
            <MetricCard
              label="Unpriced requests"
              value={String(report.totals.unpriced_request_count)}
            />
            <MetricCard
              label="Usage-missing requests"
              value={String(report.totals.usage_missing_request_count)}
            />
          </div>

          <div className="flex flex-col gap-3 rounded-md border border-[color:var(--color-border)] p-4">
            {report.daily.map((point) => {
              const amount = point.priced_cost_usd_10000 / 10_000
              const barWidth = maxDaily > 0 ? (amount / maxDaily) * 100 : 0
              return (
                <div
                  key={point.day_start}
                  className="grid grid-cols-[120px_1fr_130px] items-center gap-3"
                >
                  <span className="truncate text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                    {new Date(point.day_start).toLocaleDateString('en-US', {
                      month: 'short',
                      day: 'numeric',
                    })}
                  </span>
                  <div className="h-3 rounded-full bg-[color:var(--color-surface-muted)]">
                    <div
                      className="h-3 rounded-full bg-[var(--color-primary)]"
                      style={{ width: `${barWidth}%` }}
                    />
                  </div>
                  <span className="text-right text-sm font-semibold text-[var(--color-text)]">
                    {CURRENCY_FORMATTER.format(amount)}
                  </span>
                </div>
              )
            })}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Owner Breakdown</CardTitle>
          <CardDescription>Spend by specific user and team ownership scopes.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
            <div className="grid grid-cols-[120px_minmax(0,1fr)_170px_120px_120px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
              <span className="px-3 py-2 font-semibold">Owner type</span>
              <span className="px-3 py-2 font-semibold">Owner</span>
              <span className="px-3 py-2 font-semibold">Priced spend</span>
              <span className="px-3 py-2 font-semibold">Unpriced</span>
              <span className="px-3 py-2 font-semibold">Usage missing</span>
            </div>
            {report.owners.length === 0 ? (
              <div className="px-3 py-8 text-sm text-[var(--color-text-soft)]">
                No owner spend in this window.
              </div>
            ) : (
              report.owners.map((owner) => (
                <div
                  key={`${owner.owner_kind}:${owner.owner_id}`}
                  className="grid grid-cols-[120px_minmax(0,1fr)_170px_120px_120px] border-t border-[color:var(--color-border)]"
                >
                  <span className="px-3 py-3">
                    <Badge>{owner.owner_kind}</Badge>
                  </span>
                  <span className="truncate px-3 py-3 text-sm text-[var(--color-text)]">
                    {owner.owner_name}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text)]">
                    {CURRENCY_FORMATTER.format(owner.priced_cost_usd_10000 / 10_000)}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                    {owner.unpriced_request_count}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                    {owner.usage_missing_request_count}
                  </span>
                </div>
              ))
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Model Breakdown</CardTitle>
          <CardDescription>Priced spend and pricing gaps by canonical model key.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
            <div className="grid grid-cols-[minmax(0,1fr)_170px_120px_120px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
              <span className="px-3 py-2 font-semibold">Model</span>
              <span className="px-3 py-2 font-semibold">Priced spend</span>
              <span className="px-3 py-2 font-semibold">Unpriced</span>
              <span className="px-3 py-2 font-semibold">Usage missing</span>
            </div>
            {report.models.length === 0 ? (
              <div className="px-3 py-8 text-sm text-[var(--color-text-soft)]">
                No model spend in this window.
              </div>
            ) : (
              report.models.map((model) => (
                <div
                  key={model.model_key}
                  className="grid grid-cols-[minmax(0,1fr)_170px_120px_120px] border-t border-[color:var(--color-border)]"
                >
                  <span className="truncate px-3 py-3 text-sm font-medium text-[var(--color-text)]">
                    {model.model_key}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text)]">
                    {CURRENCY_FORMATTER.format(model.priced_cost_usd_10000 / 10_000)}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                    {model.unpriced_request_count}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                    {model.usage_missing_request_count}
                  </span>
                </div>
              ))
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-3">
      <p className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
        {label}
      </p>
      <p className="mt-1 text-lg font-semibold text-[var(--color-text)]">{value}</p>
    </div>
  )
}

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}
