import { useState, useTransition } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { Area, AreaChart, CartesianGrid, XAxis } from 'recharts'
import { toast } from 'sonner'

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
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  getObservabilityLeaderboard,
  refreshObservabilityLeaderboard,
} from '@/server/admin-data.functions'
import type { LeaderboardRange, LeaderboardView } from '@/types/api'

export const Route = createFileRoute('/observability/leaderboard')({
  beforeLoad: ({ location }) => requireAdminSession(location),
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

export function ObservabilityLeaderboardPage() {
  const loaderData = Route.useLoaderData()
  const [leaderboard, setLeaderboard] = useState<LeaderboardView>(loaderData.data)
  const [range, setRange] = useState<LeaderboardRange>(loaderData.data.range)
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
    setRange(nextRange)
    setIsRefreshing(true)

    startTransition(async () => {
      try {
        const response = await refreshObservabilityLeaderboard({
          data: {
            range: nextRange,
          },
        })
        setLeaderboard(response.data)
      } catch (error) {
        toast.error(getErrorMessage(error))
      } finally {
        setIsRefreshing(false)
      }
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
          <div className="flex flex-col gap-1">
            <CardTitle>Leaderboard</CardTitle>
            <CardDescription>
              Compare the top five users by spend over time, bucketed into UTC 12-hour windows.
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
            Ranked by total spend for the selected range with the dominant model by request volume.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <LeaderboardTableSkeleton />
          ) : leaderboard.leaders.length === 0 ? (
            <LeaderboardEmptyState />
          ) : (
            <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
              <Table data-testid="leaderboard-table">
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-16">Rank</TableHead>
                    <TableHead>User</TableHead>
                    <TableHead className="text-right">Total spend</TableHead>
                    <TableHead>Most used model</TableHead>
                    <TableHead className="text-right">Total requests</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {leaderboard.leaders.map((leader, index) => (
                    <TableRow key={leader.user_id}>
                      <TableCell className="font-medium text-[var(--color-text-soft)]">
                        {index + 1}
                      </TableCell>
                      <TableCell className="font-medium">{leader.user_name}</TableCell>
                      <TableCell className="text-right">
                        {CURRENCY_FORMATTER.format(leader.total_spend_usd_10000 / 10_000)}
                      </TableCell>
                      <TableCell>{leader.most_used_model ?? '—'}</TableCell>
                      <TableCell className="text-right">
                        {NUMBER_FORMATTER.format(leader.total_requests)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
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

function LeaderboardTableSkeleton() {
  return (
    <div className="flex flex-col gap-3" data-testid="leaderboard-table-skeleton">
      {Array.from({ length: 6 }).map((_, index) => (
        <Skeleton key={index} className="h-11 w-full rounded-md" />
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

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}
