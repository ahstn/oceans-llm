import { useState, useTransition } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { Area, AreaChart, CartesianGrid, XAxis } from 'recharts'
import { toast } from 'sonner'

import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import { PageHeader } from '@/components/layout/page-header'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  getObservabilityLeaderboard,
  refreshObservabilityLeaderboard,
} from '@/server/admin-data.functions'
import type { LeaderboardRange, LeaderboardLeaderView, LeaderboardView } from '@/types/api'

export const Route = createFileRoute('/observability/leaderboard')({
  loader: () => getObservabilityLeaderboard({ data: { range: '7d' } }),
  component: ObservabilityLeaderboardPage,
})

const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

const NUMBER_FORMATTER = new Intl.NumberFormat('en-US')

// Keep chart and ranking projections with their shared range and leaderboard data derivation.
// oxlint-disable-next-line eslint/max-lines-per-function
export function ObservabilityLeaderboardPage() {
  const loaderData = Route.useLoaderData()
  const [leaderboard, setLeaderboard] = useState<LeaderboardView>(loaderData.data)
  const [range, setRange] = useState<LeaderboardRange>(toLeaderboardRange(loaderData.data.range))
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [isPending, startTransition] = useTransition()
  const isLoading = isPending || isRefreshing

  const chartSeries = leaderboard.chart_users.map((user, index) => ({
    ...user,
    key: `user_${index + 1}`,
    color: `var(--chart-${index + 1})`,
    gradientId: `leaderboard-fill-${index + 1}`,
  }))

  const chartConfig = chartSeries.reduce<ChartConfig>((config, user) => {
    config[user.key] = {
      label: user.user_name,
      color: user.color,
    }
    return config
  }, {})

  const chartData = leaderboard.series.map((point) => {
    const row: Record<string, number | string> = {
      bucket_start: point.bucket_start,
    }

    for (const user of chartSeries) {
      row[user.key] = 0
    }

    for (const value of point.values) {
      const matchingUser = chartSeries.find((user) => user.user_id === value.user_id)
      if (matchingUser) {
        row[matchingUser.key] = value.spend_usd_10000 / 10_000
      }
    }

    return row
  })

  function refreshRange(nextRange: LeaderboardRange) {
    setIsRefreshing(true)

    startTransition(async () => {
      try {
        const response = await refreshObservabilityLeaderboard({
          data: {
            range: nextRange,
          },
        })
        setLeaderboard(response.data)
        setRange(response.data.range)
      } catch (error) {
        toast.error(getErrorMessage(error))
      } finally {
        setIsRefreshing(false)
      }
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Budget & Spending"
        title="Leaderboard"
        description="Compare user costs over time and see which users have the highest total cost."
      />

      <Card>
        <CardHeader className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
          <div className="flex flex-col gap-1">
            <CardTitle>User costs over time</CardTitle>
            <CardDescription>
              Compare 12-hour costs for the five users with the highest total cost.
            </CardDescription>
          </div>
          <ToggleGroup
            type="single"
            value={range}
            onValueChange={(value) => {
              if (value === '7d' || value === '31d') {
                refreshRange(value)
              }
            }}
            disabled={isLoading}
            className="justify-start lg:justify-end"
          >
            <ToggleGroupItem value="7d" aria-label="Last 7 days">
              7d
            </ToggleGroupItem>
            <ToggleGroupItem value="31d" aria-label="Last 31 days">
              31d
            </ToggleGroupItem>
          </ToggleGroup>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <LeaderboardChartSkeleton />
          ) : chartSeries.length === 0 ? (
            <LeaderboardEmptyState />
          ) : (
            <ChartContainer config={chartConfig} className="h-[24rem] w-full">
              <AreaChart accessibilityLayer data={chartData} margin={{ left: 12, right: 12 }}>
                <defs>
                  {chartSeries.map((user) => (
                    <linearGradient
                      key={user.gradientId}
                      id={user.gradientId}
                      x1="0"
                      y1="0"
                      x2="0"
                      y2="1"
                    >
                      <stop offset="5%" stopColor={user.color} stopOpacity={0.35} />
                      <stop offset="95%" stopColor={user.color} stopOpacity={0.04} />
                    </linearGradient>
                  ))}
                </defs>
                <CartesianGrid vertical={false} />
                <XAxis
                  dataKey="bucket_start"
                  tickLine={false}
                  axisLine={false}
                  minTickGap={24}
                  tickFormatter={(value) => formatAxisTick(value)}
                />
                <ChartTooltip
                  cursor={false}
                  content={
                    <ChartTooltipContent
                      labelFormatter={(_, payload) =>
                        formatTooltipLabel(String(payload?.[0]?.payload?.bucket_start ?? ''))
                      }
                      formatter={(value, name) => (
                        <>
                          <span className="text-muted-foreground">
                            {chartConfig[String(name)]?.label ?? String(name)}
                          </span>
                          <span className="font-mono font-medium tabular-nums">
                            {CURRENCY_FORMATTER.format(Number(value))}
                          </span>
                        </>
                      )}
                    />
                  }
                />
                <ChartLegend content={<ChartLegendContent />} />
                {chartSeries.map((user) => (
                  <Area
                    key={user.user_id}
                    dataKey={user.key}
                    type="monotone"
                    stroke={user.color}
                    fill={`url(#${user.gradientId})`}
                    fillOpacity={1}
                    strokeWidth={2}
                    stackId={undefined}
                  />
                ))}
              </AreaChart>
            </ChartContainer>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Top Users</CardTitle>
          <CardDescription>
            Ranked by total spend for the selected range with dominant model and harness, request
            volume, and average tool exposure and invocation counts.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <LeaderboardTableSkeleton />
          ) : leaderboard.leaders.length === 0 ? (
            <LeaderboardEmptyState />
          ) : (
            <LeaderboardRankings leaders={leaderboard.leaders} />
          )}
        </CardContent>
      </Card>
    </div>
  )
}

// Keep mobile and desktop projections together so leaderboard fields cannot drift.
// oxlint-disable-next-line eslint/max-lines-per-function
function LeaderboardRankings({ leaders }: { leaders: LeaderboardLeaderView[] }) {
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:hidden" data-testid="leaderboard-mobile-list">
        {leaders.map((leader) => (
          <article
            key={leader.user_id}
            className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <p className="text-xs font-semibold text-[var(--color-text-soft)]">
                  Rank {leader.rank}
                </p>
                <p className="truncate font-semibold text-[var(--color-text)]">
                  {leader.user_name}
                </p>
              </div>
              <p className="shrink-0 font-medium text-[var(--color-text)] tabular-nums">
                {CURRENCY_FORMATTER.format(leader.total_spend_usd_10000 / 10_000)}
              </p>
            </div>

            <dl className="mt-4 grid grid-cols-2 gap-3 text-sm">
              <div className="col-span-2">
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Most used model
                </dt>
                <dd className="mt-1">{leader.most_used_model ?? '—'}</dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Most used harness
                </dt>
                <dd className="mt-1">
                  <MostUsedHarness leader={leader} />
                </dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Total requests
                </dt>
                <dd className="mt-1 tabular-nums">
                  {NUMBER_FORMATTER.format(leader.total_requests)}
                </dd>
              </div>
              <div className="col-span-2">
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Avg tools
                </dt>
                <dd className="mt-1">
                  <ToolAverages leader={leader} />
                </dd>
              </div>
            </dl>
          </article>
        ))}
      </div>

      <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
        <Table data-testid="leaderboard-table" className="min-w-[80rem] text-left">
          <TableHeader className="bg-[color:var(--color-surface-muted)]">
            <TableRow>
              <TableHead className="w-16 px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Rank
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                User
              </TableHead>
              <TableHead className="px-3 py-2 text-right font-semibold text-[var(--color-text-soft)]">
                Total spend
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Most used model
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Most used harness
              </TableHead>
              <TableHead className="px-3 py-2 text-right font-semibold text-[var(--color-text-soft)]">
                Total requests
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Avg tools
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {leaders.map((leader) => (
              <TableRow key={leader.user_id}>
                <TableCell className="px-3 py-3 font-medium text-[var(--color-text-soft)]">
                  {leader.rank}
                </TableCell>
                <TableCell className="px-3 py-3 font-medium">{leader.user_name}</TableCell>
                <TableCell className="px-3 py-3 text-right tabular-nums">
                  {CURRENCY_FORMATTER.format(leader.total_spend_usd_10000 / 10_000)}
                </TableCell>
                <TableCell className="px-3 py-3">{leader.most_used_model ?? '—'}</TableCell>
                <TableCell className="px-3 py-3">
                  <MostUsedHarness leader={leader} />
                </TableCell>
                <TableCell className="px-3 py-3 text-right tabular-nums">
                  {NUMBER_FORMATTER.format(leader.total_requests)}
                </TableCell>
                <TableCell className="px-3 py-3">
                  <ToolAverages leader={leader} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function MostUsedHarness({ leader }: { leader: LeaderboardLeaderView }) {
  if (!leader.most_used_harness) {
    return <span className="text-[var(--color-text-soft)]">—</span>
  }

  return (
    <AgentHarnessLabel harnessKey={leader.most_used_harness.key}>
      {leader.most_used_harness.label}
    </AgentHarnessLabel>
  )
}

function LeaderboardChartSkeleton() {
  return (
    <div className="flex flex-col gap-3" data-testid="leaderboard-chart-skeleton">
      <Skeleton className="h-[24rem] w-full rounded-xl" />
      <div className="flex gap-3">
        <Skeleton className="h-4 w-24" />
        <Skeleton className="h-4 w-24" />
        <Skeleton className="h-4 w-24" />
      </div>
    </div>
  )
}

const leaderboardSkeletonRows = Array.from({ length: 6 }, (_, index) => `leaderboard-row-${index}`)

function LeaderboardTableSkeleton() {
  return (
    <div className="flex flex-col gap-3" data-testid="leaderboard-table-skeleton">
      {leaderboardSkeletonRows.map((row) => (
        <Skeleton key={row} className="h-11 w-full rounded-md" />
      ))}
    </div>
  )
}

function LeaderboardEmptyState() {
  return (
    <Empty className="rounded-xl border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)]">
      <EmptyHeader>
        <EmptyTitle>No leaderboard data yet</EmptyTitle>
        <EmptyDescription>
          Usage will appear here once priced or unpriced ledger events exist in the selected range.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  )
}

function ToolAverages({ leader }: { leader: LeaderboardLeaderView }) {
  const averages = leader.tool_cardinality_averages

  return (
    <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-xs text-[var(--color-text-muted)]">
      <span className="tabular-nums">
        MCP {formatAverageCount(averages.referenced_mcp_server_count)}
      </span>
      <span className="tabular-nums">
        exposed {formatAverageCount(averages.exposed_tool_count)}
      </span>
      <span className="tabular-nums">called {formatAverageCount(averages.invoked_tool_count)}</span>
      <span className="tabular-nums">
        filtered {formatAverageCount(averages.filtered_tool_count)}
      </span>
    </div>
  )
}

function formatAverageCount(value: number | null | undefined) {
  if (value == null) {
    return 'n/a'
  }

  return Number.isInteger(value) ? String(value) : value.toFixed(1)
}

function formatAxisTick(value: string) {
  if (!value) {
    return ''
  }

  return new Date(value).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  })
}

function formatTooltipLabel(value: string) {
  if (!value) {
    return 'UTC'
  }

  return new Date(value).toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    hour12: false,
    timeZone: 'UTC',
  })
}

function toLeaderboardRange(value: string): LeaderboardRange {
  return value === '31d' ? '31d' : '7d'
}

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}
