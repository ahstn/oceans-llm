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
  getObservabilityHarnessUsage,
  refreshObservabilityHarnessUsage,
} from '@/server/admin-data.functions'
import type { HarnessUsageLeaderView, HarnessUsageRange, HarnessUsageView } from '@/types/api'

export const Route = createFileRoute('/observability/agent-harnesses')({
  loader: () => getObservabilityHarnessUsage({ data: { range: '7d' } }),
  component: AgentHarnessesPage,
})

const NUMBER_FORMATTER = new Intl.NumberFormat('en-US')

// Keep chart and ranking projections with their shared range and harness data derivation.
// oxlint-disable-next-line eslint/max-lines-per-function
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
      label: (
        <AgentHarnessLabel harnessKey={harness.agent_harness_key} iconSize={14}>
          {harness.agent_harness_label}
        </AgentHarnessLabel>
      ),
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
    setIsRefreshing(true)

    startTransition(async () => {
      try {
        const response = await refreshObservabilityHarnessUsage({
          data: {
            range: nextRange,
          },
        })
        setUsage(response.data)
        setRange(toHarnessUsageRange(response.data.range))
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
        section="Observability"
        title="Agent harnesses"
        description="Compare request activity from user harnesses over time."
      />

      <Card>
        <CardHeader className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
          <div className="flex flex-col gap-1">
            <CardTitle>Request activity</CardTitle>
            <CardDescription>
              Compare request counts for each harness in 12-hour periods.
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
            Ranked by request count for the selected range with input, output, and total tokens.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <HarnessTableSkeleton />
          ) : usage.leaders.length === 0 ? (
            <HarnessEmptyState />
          ) : (
            <HarnessRankings leaders={usage.leaders} />
          )}
        </CardContent>
      </Card>
    </div>
  )
}

// Keep mobile and desktop projections together so harness metrics cannot drift.
// oxlint-disable-next-line eslint/max-lines-per-function
function HarnessRankings({ leaders }: { leaders: HarnessUsageLeaderView[] }) {
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:hidden" data-testid="harness-usage-mobile-list">
        {leaders.map((leader, index) => (
          <article
            key={leader.agent_harness_key}
            className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
          >
            <div className="flex items-start gap-3">
              <span className="text-xs font-semibold text-[var(--color-text-soft)]">
                Rank {index + 1}
              </span>
              <div className="min-w-0">
                <p className="font-semibold text-[var(--color-text)]">
                  <AgentHarnessLabel harnessKey={leader.agent_harness_key}>
                    {leader.agent_harness_label}
                  </AgentHarnessLabel>
                </p>
                <p className="truncate font-mono text-xs text-[var(--color-text-soft)]">
                  {leader.agent_harness_key}
                </p>
              </div>
            </div>

            <dl className="mt-4 grid grid-cols-2 gap-3 text-sm">
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Requests
                </dt>
                <dd className="mt-1 tabular-nums">
                  {NUMBER_FORMATTER.format(leader.total_requests)}
                </dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Input tokens
                </dt>
                <dd className="mt-1 tabular-nums">{formatTokenCount(leader.prompt_tokens)}</dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Output tokens
                </dt>
                <dd className="mt-1 tabular-nums">{formatTokenCount(leader.completion_tokens)}</dd>
              </div>
              <div>
                <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                  Total tokens
                </dt>
                <dd className="mt-1 tabular-nums">{formatTokenCount(leader.total_tokens)}</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>

      <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
        <Table data-testid="harness-usage-table" className="min-w-[64rem] text-left">
          <TableHeader className="bg-[color:var(--color-surface-muted)]">
            <TableRow>
              <TableHead className="w-16 px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Rank
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Harness
              </TableHead>
              <TableHead className="px-3 py-2 text-right font-semibold text-[var(--color-text-soft)]">
                Requests
              </TableHead>
              <TableHead className="px-3 py-2 text-right font-semibold text-[var(--color-text-soft)]">
                Input tokens
              </TableHead>
              <TableHead className="px-3 py-2 text-right font-semibold text-[var(--color-text-soft)]">
                Output tokens
              </TableHead>
              <TableHead className="px-3 py-2 text-right font-semibold text-[var(--color-text-soft)]">
                Total tokens
              </TableHead>
              <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                Key
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {leaders.map((leader, index) => (
              <TableRow key={leader.agent_harness_key}>
                <TableCell className="px-3 py-3 font-medium text-[var(--color-text-soft)]">
                  {index + 1}
                </TableCell>
                <TableCell className="px-3 py-3 font-medium">
                  <AgentHarnessLabel harnessKey={leader.agent_harness_key}>
                    {leader.agent_harness_label}
                  </AgentHarnessLabel>
                </TableCell>
                <TableCell className="px-3 py-3 text-right tabular-nums">
                  {NUMBER_FORMATTER.format(leader.total_requests)}
                </TableCell>
                <TableCell className="px-3 py-3 text-right tabular-nums">
                  {formatTokenCount(leader.prompt_tokens)}
                </TableCell>
                <TableCell className="px-3 py-3 text-right tabular-nums">
                  {formatTokenCount(leader.completion_tokens)}
                </TableCell>
                <TableCell className="px-3 py-3 text-right tabular-nums">
                  {formatTokenCount(leader.total_tokens)}
                </TableCell>
                <TableCell className="px-3 py-3 font-mono text-xs text-[var(--color-text-muted)]">
                  {leader.agent_harness_key}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function formatTokenCount(value: number | null | undefined) {
  return value == null ? 'n/a' : NUMBER_FORMATTER.format(value)
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
    timeZone: 'UTC',
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
    timeZone: 'UTC',
  }).format(date)
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Harness usage could not be refreshed.'
}

function toHarnessUsageRange(value: string): HarnessUsageRange {
  return value === '31d' ? '31d' : '7d'
}
