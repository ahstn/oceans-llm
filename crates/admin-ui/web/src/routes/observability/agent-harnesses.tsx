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
  getObservabilityHarnessUsage,
  refreshObservabilityHarnessUsage,
} from '@/server/admin-data.functions'
import type { HarnessUsageRange, HarnessUsageView } from '@/types/api'

export const Route = createFileRoute('/observability/agent-harnesses')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getObservabilityHarnessUsage({ data: { range: '7d' } }),
  component: AgentHarnessesPage,
})

const NUMBER_FORMATTER = new Intl.NumberFormat('en-US')

export function AgentHarnessesPage() {
  const loaderData = Route.useLoaderData()
  const [usage, setUsage] = useState<HarnessUsageView>(loaderData.data)
  const [range, setRange] = useState<HarnessUsageRange>(toHarnessUsageRange(loaderData.data.range))
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [isPending, startTransition] = useTransition()
  const isLoading = isPending || isRefreshing

  const chartSeries = usage.chart_harnesses.map((harness, index) => ({
    ...harness,
    key: `harness_${index + 1}`,
    color: `var(--chart-${index + 1})`,
    gradientId: `harness-fill-${index + 1}`,
  }))

  const chartConfig = chartSeries.reduce<ChartConfig>((config, harness) => {
    config[harness.key] = {
      label: harness.agent_harness_label,
      color: harness.color,
    }
    return config
  }, {})
  const chartKeyByHarnessKey = new Map(
    chartSeries.map((harness) => [harness.agent_harness_key, harness.key]),
  )

  const chartData = usage.series.map((point) => {
    const row: Record<string, number | string> = {
      bucket_start: point.bucket_start,
    }

    for (const harness of chartSeries) {
      row[harness.key] = 0
    }

    for (const value of point.values) {
      const chartKey = chartKeyByHarnessKey.get(value.agent_harness_key)
      if (chartKey) {
        row[chartKey] = value.request_count
      }
    }

    return row
  })

  function refreshRange(nextRange: HarnessUsageRange) {
    setRange(nextRange)
    setIsRefreshing(true)

    startTransition(async () => {
      try {
        const response = await refreshObservabilityHarnessUsage({
          data: {
            range: nextRange,
          },
        })
        setUsage(response.data)
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
            <CardTitle>Agent Harnesses</CardTitle>
            <CardDescription>
              Compare self-reported User-Agent harness traffic over UTC 12-hour windows.
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
            <HarnessChartSkeleton />
          ) : chartSeries.length === 0 ? (
            <HarnessEmptyState />
          ) : (
            <ChartContainer config={chartConfig} className="h-[24rem] w-full">
              <AreaChart accessibilityLayer data={chartData} margin={{ left: 12, right: 12 }}>
                <defs>
                  {chartSeries.map((harness) => (
                    <linearGradient
                      key={harness.gradientId}
                      id={harness.gradientId}
                      x1="0"
                      y1="0"
                      x2="0"
                      y2="1"
                    >
                      <stop offset="5%" stopColor={harness.color} stopOpacity={0.35} />
                      <stop offset="95%" stopColor={harness.color} stopOpacity={0.04} />
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
                            {NUMBER_FORMATTER.format(Number(value))} requests
                          </span>
                        </>
                      )}
                    />
                  }
                />
                <ChartLegend content={<ChartLegendContent />} />
                {chartSeries.map((harness) => (
                  <Area
                    key={harness.agent_harness_key}
                    dataKey={harness.key}
                    type="monotone"
                    stroke={harness.color}
                    fill={`url(#${harness.gradientId})`}
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
          <CardTitle>Top Harnesses</CardTitle>
          <CardDescription>
            Ranked by request count for the selected range using normalized harness labels.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <HarnessTableSkeleton />
          ) : usage.leaders.length === 0 ? (
            <HarnessEmptyState />
          ) : (
            <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
              <Table data-testid="harness-usage-table">
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-16">Rank</TableHead>
                    <TableHead>Harness</TableHead>
                    <TableHead className="text-right">Requests</TableHead>
                    <TableHead>Key</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {usage.leaders.map((leader, index) => (
                    <TableRow key={leader.agent_harness_key}>
                      <TableCell className="font-medium text-[var(--color-text-soft)]">
                        {index + 1}
                      </TableCell>
                      <TableCell className="font-medium">{leader.agent_harness_label}</TableCell>
                      <TableCell className="text-right">
                        {NUMBER_FORMATTER.format(leader.total_requests)}
                      </TableCell>
                      <TableCell className="font-mono text-xs text-[var(--color-text-muted)]">
                        {leader.agent_harness_key}
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

function HarnessEmptyState() {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>No harness data yet</EmptyTitle>
        <EmptyDescription>
          Harness usage appears after request logging captures gateway requests.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  )
}

function HarnessChartSkeleton() {
  return <Skeleton data-testid="harness-chart-skeleton" className="h-[24rem] w-full" />
}

function HarnessTableSkeleton() {
  return <Skeleton data-testid="harness-table-skeleton" className="h-48 w-full" />
}

function formatAxisTick(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
  }).format(date)
}

function formatTooltipLabel(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat('en-US', {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
  }).format(date)
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Harness usage could not be refreshed.'
}

function toHarnessUsageRange(value: string): HarnessUsageRange {
  return value === '31d' ? '31d' : '7d'
}
