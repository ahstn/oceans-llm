import { lazy, Suspense } from 'react'
import { createFileRoute } from '@tanstack/react-router'

import { PageHeader } from '@/components/layout/page-header'
import { Badge } from '@/components/ui/badge'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import type { ChartConfig } from '@/components/ui/chart'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Progress } from '@/components/ui/progress'
import { Skeleton } from '@/components/ui/skeleton'
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import { getUsageCosts } from '@/server/admin-data.functions'
import type { SpendReportView } from '@/types/api'

import { ExportMenu, ReportFilters } from './-usage-costs/controls'
import {
  CURRENCY_FORMATTER,
  formatCount,
  formatDay,
  formatShare,
  formatUsd,
  pricingCoverage,
  PERCENT_FORMATTER,
  totalRequests,
  useSpendReport,
} from './-usage-costs/shared'

export const Route = createFileRoute('/observability/usage-costs')({
  loader: () => getUsageCosts(),
  component: UsageCostsPage,
})

/** Breakdown cards show the top spenders only; the FOCUS export carries the full list. */
const BREAKDOWN_LIMIT = 10

const CHART_CONFIG: ChartConfig = {
  cost: { label: 'Priced spend', color: 'var(--chart-3)' },
}
type SpendChartDatum = {
  day: string
  cost: number
  requests: number
}

const SpendTrendChart = lazy(async () => {
  const [
    { Area, AreaChart, CartesianGrid, XAxis, YAxis },
    { ChartContainer, ChartTooltip, ChartTooltipContent },
  ] = await Promise.all([import('recharts'), import('@/components/ui/chart')])

  function SpendTrendChartComponent({ data }: { data: SpendChartDatum[] }) {
    return (
      <ChartContainer config={CHART_CONFIG} className="h-64 w-full">
        <AreaChart accessibilityLayer data={data} margin={{ left: 4, right: 12 }}>
          <defs>
            <linearGradient id="usage-costs-fill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="var(--color-cost)" stopOpacity={0.35} />
              <stop offset="95%" stopColor="var(--color-cost)" stopOpacity={0.04} />
            </linearGradient>
          </defs>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            minTickGap={24}
            tickFormatter={formatDay}
          />
          <YAxis
            tickLine={false}
            axisLine={false}
            width={56}
            tickFormatter={(value: number) => CURRENCY_FORMATTER.format(value)}
          />
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) => formatDay(String(payload?.[0]?.payload?.day ?? ''))}
                formatter={(value, _name, item) => (
                  <div className="flex w-full items-center justify-between gap-4">
                    <span className="text-muted-foreground">
                      {item.payload.requests} priced requests
                    </span>
                    <span className="font-mono font-medium tabular-nums">
                      {CURRENCY_FORMATTER.format(Number(value))}
                    </span>
                  </div>
                )}
              />
            }
          />
          <Area
            dataKey="cost"
            type="monotone"
            stroke="var(--color-cost)"
            fill="url(#usage-costs-fill)"
            strokeWidth={2}
          />
        </AreaChart>
      </ChartContainer>
    )
  }

  return { default: SpendTrendChartComponent }
})

// Executive dashboard: KPI strip, spend trend, and ranked share tables.
// oxlint-disable-next-line eslint/max-lines-per-function
export function UsageCostsPage() {
  const loaderData = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const isPlatformAdmin = isPlatformAdminSession(session)
  const spend = useSpendReport(loaderData.data)
  const { report, isPending } = spend

  const coverage = pricingCoverage(report.totals)
  const requests = totalRequests(report.totals)
  const chartData = report.daily.map((point) => ({
    day: point.day_start,
    cost: point.priced_cost_usd_10000 / 10_000,
    requests: point.priced_request_count,
  }))
  // The API zero-fills every day in the window, so "no spend" is a value check, not a length check.
  const peakDay = report.daily.reduce<SpendReportView['daily'][number] | null>(
    (peak, point) =>
      point.priced_cost_usd_10000 > (peak?.priced_cost_usd_10000 ?? 0) ? point : peak,
    null,
  )
  const avgDaily =
    report.window_days > 0 ? report.totals.priced_cost_usd_10000 / report.window_days : 0

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Budget & Spending"
        title="Usage costs"
        description={
          isPlatformAdmin
            ? 'Review costs over time and see how each account and model affects the total.'
            : 'Review your costs over time and see how each model affects the total.'
        }
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <ReportFilters
              windowDays={spend.windowDays}
              ownerKind={spend.ownerKind}
              isPending={isPending}
              isPlatformAdmin={isPlatformAdmin}
              onWindowChange={spend.setWindowDays}
              onOwnerKindChange={spend.setOwnerKind}
              onRefresh={spend.refresh}
            />
            <ExportMenu
              windowDays={spend.windowDays}
              ownerKind={spend.ownerKind}
              isPlatformAdmin={isPlatformAdmin}
              origin={loaderData.exportOrigin}
            />
          </div>
        }
      />

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Kpi
          label="Priced spend"
          value={formatUsd(report.totals.priced_cost_usd_10000)}
          hint={`${CURRENCY_FORMATTER.format(avgDaily / 10_000)} per day on average`}
          pending={isPending}
        />
        <Kpi
          label="Requests"
          value={formatCount(requests)}
          hint={`${formatCount(report.totals.priced_request_count)} priced`}
          pending={isPending}
        />
        <Kpi
          label="Pricing coverage"
          value={PERCENT_FORMATTER.format(coverage)}
          hint={
            <span className="flex flex-col gap-1.5">
              <Progress value={coverage * 100} aria-label="Pricing coverage" />
              <span>
                {formatCount(report.totals.unpriced_request_count)} unpriced ·{' '}
                {formatCount(report.totals.usage_missing_request_count)} usage missing
              </span>
            </span>
          }
          tone={coverage < 0.95 ? 'warning' : 'default'}
          pending={isPending}
        />
        <Kpi
          label="Peak day"
          value={peakDay ? formatUsd(peakDay.priced_cost_usd_10000) : '—'}
          hint={peakDay ? formatDay(peakDay.day_start) : 'No priced spend in window'}
          pending={isPending}
        />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Spend trend</CardTitle>
          <CardDescription>Priced spend per UTC day for the selected window.</CardDescription>
          <CardAction>
            <TokenSummary totals={report.totals} />
          </CardAction>
        </CardHeader>
        <CardContent>
          {isPending ? (
            <Skeleton className="h-64 w-full rounded-xl" />
          ) : peakDay === null ? (
            <EmptyReport />
          ) : (
            <Suspense fallback={<Skeleton className="h-64 w-full rounded-xl" />}>
              <SpendTrendChart data={chartData} />
            </Suspense>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-6 xl:grid-cols-2">
        <ShareTable
          title="Owner breakdown"
          description={
            isPlatformAdmin
              ? 'Spend by user and service account ownership scopes.'
              : 'Spend attributed to your user account.'
          }
          emptyMessage="No owner spend in this window."
          total={report.totals.priced_cost_usd_10000}
          pending={isPending}
          rows={report.owners.map((owner) => ({
            key: `${owner.owner_kind}:${owner.owner_id}`,
            label: owner.owner_name,
            cost: owner.priced_cost_usd_10000,
            gaps: owner.unpriced_request_count + owner.usage_missing_request_count,
          }))}
        />
        <ShareTable
          title="Model breakdown"
          description="Priced spend and pricing gaps by canonical model key."
          emptyMessage="No model spend in this window."
          total={report.totals.priced_cost_usd_10000}
          pending={isPending}
          rows={report.models.map((model) => ({
            key: model.model_key,
            label: model.model_key,
            mono: true,
            cost: model.priced_cost_usd_10000,
            gaps: model.unpriced_request_count + model.usage_missing_request_count,
          }))}
        />
      </div>
    </div>
  )
}

function Kpi({
  label,
  value,
  hint,
  tone = 'default',
  pending,
}: {
  label: string
  value: string
  hint: React.ReactNode
  tone?: 'default' | 'warning'
  pending: boolean
}) {
  return (
    <Card size="sm">
      <CardHeader>
        <CardDescription>{label}</CardDescription>
        {pending ? (
          <Skeleton className="h-7 w-28" />
        ) : (
          <CardTitle
            className={
              tone === 'warning'
                ? 'text-2xl text-[var(--color-warning)] tabular-nums'
                : 'text-2xl tabular-nums'
            }
          >
            {value}
          </CardTitle>
        )}
      </CardHeader>
      <CardContent className="text-muted-foreground text-xs">{hint}</CardContent>
    </Card>
  )
}

function TokenSummary({ totals }: { totals: SpendReportView['totals'] }) {
  const items = [
    ['Uncached input', totals.uncached_input_tokens],
    ['Cache read', totals.cache_read_tokens],
    ['Cache write', totals.cache_write_tokens],
  ] as const
  return (
    <dl className="flex flex-wrap gap-x-5 gap-y-1 text-xs">
      {items.map(([label, value]) => (
        <div key={label} className="flex flex-col">
          <dt className="text-muted-foreground">{label}</dt>
          <dd className="font-medium tabular-nums">{formatCount(value)}</dd>
        </div>
      ))}
    </dl>
  )
}

type ShareRow = {
  key: string
  label: string
  mono?: boolean
  cost: number
  gaps: number
}

function ShareTable({
  title,
  description,
  emptyMessage,
  total,
  rows,
  pending,
}: {
  title: string
  description: string
  emptyMessage: string
  total: number
  rows: ShareRow[]
  pending: boolean
}) {
  const sorted = [...rows].sort((a, b) => b.cost - a.cost).slice(0, BREAKDOWN_LIMIT)
  const max = sorted[0]?.cost ?? 0
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        {pending ? (
          <div className="flex flex-col gap-2">
            {['a', 'b', 'c', 'd'].map((row) => (
              <Skeleton key={row} className="h-9 w-full rounded-md" />
            ))}
          </div>
        ) : sorted.length === 0 ? (
          <p className="text-muted-foreground py-6 text-center text-sm">{emptyMessage}</p>
        ) : (
          <ol className="flex flex-col gap-1">
            {sorted.map((row) => (
              <li key={row.key} className="flex flex-col gap-1.5 py-2">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className={row.mono ? 'truncate font-mono text-sm' : 'truncate text-sm'}>
                      {row.label}
                    </span>
                    {row.gaps > 0 ? (
                      <Badge variant="warning">{formatCount(row.gaps)} gaps</Badge>
                    ) : null}
                  </div>
                  <div className="flex shrink-0 items-baseline gap-2">
                    <span className="text-muted-foreground text-xs tabular-nums">
                      {formatShare(row.cost, total)}
                    </span>
                    <span className="text-sm font-medium tabular-nums">{formatUsd(row.cost)}</span>
                  </div>
                </div>
                <Progress value={max > 0 ? (row.cost / max) * 100 : 0} aria-hidden="true" />
              </li>
            ))}
          </ol>
        )}
      </CardContent>
    </Card>
  )
}

function EmptyReport() {
  return (
    <Empty className="rounded-xl border bg-[color:var(--color-surface-muted)]">
      <EmptyHeader>
        <EmptyTitle>No priced spend yet</EmptyTitle>
        <EmptyDescription>
          Daily costs appear once priced ledger events exist in the selected window.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  )
}
