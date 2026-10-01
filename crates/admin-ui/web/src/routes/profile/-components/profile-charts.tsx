import { lazy, Suspense, type ReactNode } from 'react'

import { Skeleton } from '@/components/ui/skeleton'

import {
  COMPACT_FORMATTER,
  formatShortDay,
  NUMBER_FORMATTER,
  type SeriesChart,
} from './profile-data'

// Narrow cards wrap whole legend entries instead of breaking labels mid-word.
const LEGEND_CLASS = 'flex-wrap gap-x-4 gap-y-1 [&>div]:whitespace-nowrap'

// Recharts stays out of the route chunk, matching the usage & costs charts.
const loadChartKit = () => Promise.all([import('recharts'), import('@/components/ui/chart')])

type ChartProps = {
  chart: SeriesChart
  /** Weekly buckets label their tooltip as the week they start. */
  weekly?: boolean
}

function tooltipLabel(day: string, weekly?: boolean) {
  return weekly ? `Week of ${formatShortDay(day)}` : formatShortDay(day)
}

function tooltipRow(chart: SeriesChart, name: unknown, value: string) {
  return (
    <div className="flex w-full items-center justify-between gap-4">
      <span className="text-muted-foreground">
        {chart.config[String(name)]?.label ?? String(name)}
      </span>
      <span className="font-mono font-medium tabular-nums">{value}</span>
    </div>
  )
}

/** Stacked output, uncached input and cached input tokens. */
const LazyTokenVolumeChart = lazy(async () => {
  const [
    { Area, AreaChart, CartesianGrid, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()

  function TokenVolumeChartComponent({ chart, weekly }: ChartProps) {
    return (
      <ChartContainer config={chart.config} className="h-72 w-full">
        <AreaChart accessibilityLayer data={chart.rows} margin={{ left: 4, right: 12 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            minTickGap={32}
            tickFormatter={formatShortDay}
          />
          <YAxis
            tickLine={false}
            axisLine={false}
            width={44}
            tickFormatter={(value: number) => COMPACT_FORMATTER.format(value)}
          />
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                indicator="dot"
                labelFormatter={(_, payload) =>
                  tooltipLabel(String(payload?.[0]?.payload?.day ?? ''), weekly)
                }
                formatter={(value, name) =>
                  tooltipRow(chart, name, NUMBER_FORMATTER.format(Number(value)))
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent className={LEGEND_CLASS} />} />
          {chart.keys.map((series) => (
            <Area
              key={series.key}
              dataKey={series.key}
              type="monotone"
              stackId="tokens"
              stroke={series.color}
              fill={series.color}
              fillOpacity={0.35}
              strokeWidth={1.5}
            />
          ))}
        </AreaChart>
      </ChartContainer>
    )
  }

  return { default: TokenVolumeChartComponent }
})

/** Requests per bucket stacked by model or harness. */
const LazyRequestsBarChart = lazy(async () => {
  const [
    { Bar, BarChart, CartesianGrid, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()

  function RequestsBarChartComponent({ chart, weekly }: ChartProps) {
    return (
      <ChartContainer config={chart.config} className="h-72 w-full">
        <BarChart accessibilityLayer data={chart.rows} margin={{ left: 4, right: 12 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            minTickGap={32}
            tickFormatter={formatShortDay}
          />
          <YAxis
            tickLine={false}
            axisLine={false}
            width={44}
            allowDecimals={false}
            tickFormatter={(value: number) => COMPACT_FORMATTER.format(value)}
          />
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) =>
                  tooltipLabel(String(payload?.[0]?.payload?.day ?? ''), weekly)
                }
                formatter={(value, name) =>
                  tooltipRow(chart, name, NUMBER_FORMATTER.format(Number(value)))
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent className={LEGEND_CLASS} />} />
          {chart.keys.map((series) => (
            <Bar key={series.key} dataKey={series.key} stackId="requests" fill={series.color} />
          ))}
        </BarChart>
      </ChartContainer>
    )
  }

  return { default: RequestsBarChartComponent }
})

function ChartFallback() {
  return <Skeleton className="h-72 w-full rounded-lg" />
}

function hasData(chart: SeriesChart) {
  return chart.rows.some((row) => chart.keys.some(({ key }) => Number(row[key]) > 0))
}

function ChartEmpty({ children }: { children: ReactNode }) {
  return (
    <div className="text-muted-foreground flex h-72 items-center justify-center rounded-lg border border-dashed text-sm">
      {children}
    </div>
  )
}

export function TokenVolumeChart(props: ChartProps) {
  if (!hasData(props.chart)) {
    return <ChartEmpty>No token usage in this window.</ChartEmpty>
  }
  return (
    <Suspense fallback={<ChartFallback />}>
      <LazyTokenVolumeChart {...props} />
    </Suspense>
  )
}

export function RequestsBarChart(props: ChartProps) {
  if (!hasData(props.chart)) {
    return <ChartEmpty>No requests in this window.</ChartEmpty>
  }
  return (
    <Suspense fallback={<ChartFallback />}>
      <LazyRequestsBarChart {...props} />
    </Suspense>
  )
}
