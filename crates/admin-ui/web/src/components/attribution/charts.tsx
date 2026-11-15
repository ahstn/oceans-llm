// Recharts views for the Attribution tabs and their concept mockups. Each chart loads Recharts on
// first render behind its own skeleton, so Recharts stays out of every route chunk, as on Usage Costs.

import { lazy, Suspense, type ComponentProps } from 'react'

import type { ChartConfig } from '@/components/ui/chart'
import { Skeleton } from '@/components/ui/skeleton'
import { chartAnimation } from '@/lib/chart-motion'
import { formatCount, formatUsdTotal } from '@/lib/format'

import { AI_TONE, HUMAN_TONE, SHIPPED_TONE, UNSHIPPED_TONE } from './parts'
import { PERCENT, formatDay } from './format'

const loadChartKit = () => Promise.all([import('recharts'), import('@/components/ui/chart')])

function ChartSkeleton() {
  return <Skeleton className="h-64 w-full rounded-md" />
}

const SERIES = [
  'var(--chart-1)',
  'var(--chart-2)',
  'var(--chart-3)',
  'var(--chart-4)',
  'var(--chart-5)',
]

function tooltipRow(label: string, value: string) {
  return (
    <div className="flex w-full items-center justify-between gap-4">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono font-medium tabular-nums">{value}</span>
    </div>
  )
}

const LINES_CONFIG = {
  aiLines: { label: 'AI lines', color: AI_TONE },
  humanLines: { label: 'Human lines', color: HUMAN_TONE },
  aiShare: { label: 'AI share', color: 'var(--chart-4)' },
} satisfies ChartConfig

/** AI and human lines stacked, with AI share as a line on its own axis. */
const LazyLinesMixChart = lazy(async () => {
  const [
    { Area, CartesianGrid, ComposedChart, Line, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()
  function LinesMixChart({
    rows,
  }: {
    rows: { day: string; aiLines: number; humanLines: number; aiShare: number | null }[]
  }) {
    return (
      <ChartContainer config={LINES_CONFIG} className="aspect-auto h-64 w-full">
        <ComposedChart data={rows} margin={{ left: 4, right: 4 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            minTickGap={28}
            tickFormatter={formatDay}
          />
          <YAxis
            yAxisId="lines"
            tickLine={false}
            axisLine={false}
            width={44}
            tickFormatter={(value: number) => formatCount(value)}
          />
          <YAxis
            yAxisId="share"
            orientation="right"
            domain={[0, 1]}
            tickLine={false}
            axisLine={false}
            width={40}
            tickFormatter={(value: number) => PERCENT.format(value)}
          />
          <ChartTooltip
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) => formatDay(String(payload?.[0]?.payload?.day ?? ''))}
                formatter={(value, name) =>
                  tooltipRow(
                    LINES_CONFIG[name as keyof typeof LINES_CONFIG]?.label ?? String(name),
                    name === 'aiShare' ? PERCENT.format(Number(value)) : formatCount(Number(value)),
                  )
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          <Area
            yAxisId="lines"
            dataKey="humanLines"
            stackId="lines"
            type="monotone"
            fill="var(--color-humanLines)"
            fillOpacity={0.35}
            stroke="var(--color-humanLines)"
            {...chartAnimation}
          />
          <Area
            yAxisId="lines"
            dataKey="aiLines"
            stackId="lines"
            type="monotone"
            fill="var(--color-aiLines)"
            fillOpacity={0.45}
            stroke="var(--color-aiLines)"
            {...chartAnimation}
          />
          <Line
            yAxisId="share"
            dataKey="aiShare"
            type="monotone"
            stroke="var(--color-aiShare)"
            strokeWidth={2}
            dot={false}
            strokeDasharray="4 3"
            connectNulls
            {...chartAnimation}
          />
        </ComposedChart>
      </ChartContainer>
    )
  }
  return { default: LinesMixChart }
})

export function LinesMixChart(props: ComponentProps<typeof LazyLinesMixChart>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazyLinesMixChart {...props} />
    </Suspense>
  )
}

const SPEND_CONFIG = {
  committedCost: { label: 'Allocated to commits', color: SHIPPED_TONE },
  uncommittedCost: { label: 'After the last commit', color: UNSHIPPED_TONE },
  mergedPrs: { label: 'Merged PRs', color: 'var(--chart-2)' },
} satisfies ChartConfig

/** Spend per bucket, split by whether it reached a commit, against merged PRs. */
const LazySpendOutputChart = lazy(async () => {
  const [
    { Bar, CartesianGrid, ComposedChart, Line, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()
  function SpendOutputChart({
    rows,
  }: {
    rows: { day: string; committedCost: number; uncommittedCost: number; mergedPrs: number }[]
  }) {
    return (
      <ChartContainer config={SPEND_CONFIG} className="aspect-auto h-64 w-full">
        <ComposedChart data={rows} margin={{ left: 4, right: 4 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            minTickGap={28}
            tickFormatter={formatDay}
          />
          <YAxis
            yAxisId="cost"
            tickLine={false}
            axisLine={false}
            width={52}
            tickFormatter={(value: number) => `$${formatCount(value)}`}
          />
          <YAxis
            yAxisId="prs"
            orientation="right"
            allowDecimals={false}
            tickLine={false}
            axisLine={false}
            width={32}
          />
          <ChartTooltip
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) => formatDay(String(payload?.[0]?.payload?.day ?? ''))}
                formatter={(value, name) =>
                  tooltipRow(
                    SPEND_CONFIG[name as keyof typeof SPEND_CONFIG]?.label ?? String(name),
                    name === 'mergedPrs'
                      ? formatCount(Number(value))
                      : formatUsdTotal(Number(value)),
                  )
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          <Bar
            yAxisId="cost"
            dataKey="committedCost"
            stackId="cost"
            fill="var(--color-committedCost)"
            radius={[0, 0, 0, 0]}
            {...chartAnimation}
          />
          <Bar
            yAxisId="cost"
            dataKey="uncommittedCost"
            stackId="cost"
            fill="var(--color-uncommittedCost)"
            radius={[3, 3, 0, 0]}
            {...chartAnimation}
          />
          <Line
            yAxisId="prs"
            dataKey="mergedPrs"
            type="monotone"
            stroke="var(--color-mergedPrs)"
            strokeWidth={2}
            dot={false}
            {...chartAnimation}
          />
        </ComposedChart>
      </ChartContainer>
    )
  }
  return { default: SpendOutputChart }
})

export function SpendOutputChart(props: ComponentProps<typeof LazySpendOutputChart>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazySpendOutputChart {...props} />
    </Suspense>
  )
}

const LEAD_CONFIG = {
  p50: { label: 'Median', color: 'var(--chart-1)' },
  p85: { label: '85th percentile', color: 'var(--chart-5)' },
} satisfies ChartConfig

/** Lead time percentiles per bucket. */
const LazyLeadTimeTrendChart = lazy(async () => {
  const [
    { CartesianGrid, Line, LineChart, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()
  function LeadTimeTrendChart({
    rows,
  }: {
    rows: { day: string; p50: number | null; p85: number | null }[]
  }) {
    // A week or two of merges is one or two points; dots keep them visible without a line.
    const sparse = rows.filter((row) => row.p50 != null).length < 3
    return (
      <ChartContainer config={LEAD_CONFIG} className="aspect-auto h-56 w-full">
        <LineChart data={rows} margin={{ left: 4, right: 12 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            tickLine={false}
            axisLine={false}
            minTickGap={28}
            tickFormatter={formatDay}
          />
          <YAxis
            tickLine={false}
            axisLine={false}
            width={56}
            tickFormatter={(value: number) => `${Math.round(value)} h`}
          />
          <ChartTooltip
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) => formatDay(String(payload?.[0]?.payload?.day ?? ''))}
                formatter={(value, name) =>
                  tooltipRow(
                    LEAD_CONFIG[name as keyof typeof LEAD_CONFIG]?.label ?? String(name),
                    `${Number(value).toFixed(1)} h`,
                  )
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          <Line
            dataKey="p85"
            type="monotone"
            stroke="var(--color-p85)"
            strokeWidth={1.5}
            strokeDasharray="4 3"
            dot={sparse}
            connectNulls
            {...chartAnimation}
          />
          <Line
            dataKey="p50"
            type="monotone"
            stroke="var(--color-p50)"
            strokeWidth={2}
            dot={sparse}
            connectNulls
            {...chartAnimation}
          />
        </LineChart>
      </ChartContainer>
    )
  }
  return { default: LeadTimeTrendChart }
})

export function LeadTimeTrendChart(props: ComponentProps<typeof LazyLeadTimeTrendChart>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazyLeadTimeTrendChart {...props} />
    </Suspense>
  )
}

const SCATTER_CONFIG = {
  prs: { label: 'Pull requests', color: 'var(--chart-1)' },
} satisfies ChartConfig

/** One dot per merged PR: AI share across, lead time up, size by lines. */
const LazyLeadTimeScatter = lazy(async () => {
  const [
    { CartesianGrid, ReferenceLine, Scatter, ScatterChart, XAxis, YAxis, ZAxis },
    { ChartContainer, ChartTooltip },
  ] = await loadChartKit()
  function LeadTimeScatter({
    points,
    medianHours,
  }: {
    points: { aiShare: number; leadHours: number; lines: number; label: string; cost: string }[]
    medianHours: number | null
  }) {
    return (
      <ChartContainer config={SCATTER_CONFIG} className="aspect-auto h-72 w-full">
        <ScatterChart margin={{ left: 4, right: 12, top: 8 }}>
          <CartesianGrid />
          <XAxis
            type="number"
            dataKey="aiShare"
            name="AI share"
            domain={[0, 1]}
            tickLine={false}
            axisLine={false}
            tickFormatter={(value: number) => PERCENT.format(value)}
          />
          <YAxis
            type="number"
            dataKey="leadHours"
            name="Lead time"
            scale="log"
            domain={[1, 'auto']}
            allowDataOverflow
            tickLine={false}
            axisLine={false}
            width={56}
            tickFormatter={(value: number) => `${Math.round(value)} h`}
          />
          <ZAxis type="number" dataKey="lines" range={[16, 220]} />
          {medianHours ? (
            <ReferenceLine y={medianHours} stroke="var(--border)" strokeDasharray="4 3" />
          ) : null}
          <ChartTooltip
            cursor={false}
            content={({ active, payload }) => {
              const point = active ? payload?.[0]?.payload : null
              if (!point) return null
              return (
                <div className="bg-popover text-popover-foreground ring-foreground/10 shadow-popover flex min-w-48 flex-col gap-1 rounded-lg px-2.5 py-1.5 text-xs ring-1">
                  <span className="font-medium">{point.label}</span>
                  {tooltipRow('AI share', PERCENT.format(point.aiShare))}
                  {tooltipRow('Lead time', `${point.leadHours.toFixed(1)} h`)}
                  {tooltipRow('Added lines', formatCount(point.lines))}
                  {tooltipRow('Cost', point.cost)}
                </div>
              )
            }}
          />
          <Scatter data={points} fill="var(--color-prs)" fillOpacity={0.55} {...chartAnimation} />
        </ScatterChart>
      </ChartContainer>
    )
  }
  return { default: LeadTimeScatter }
})

export function LeadTimeScatter(props: ComponentProps<typeof LazyLeadTimeScatter>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazyLeadTimeScatter {...props} />
    </Suspense>
  )
}

/** Weekly gateway cost stacked by model. */
const LazyModelMixChart = lazy(async () => {
  const [
    { Bar, BarChart, CartesianGrid, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()
  function ModelMixChart({
    rows,
    models,
  }: {
    rows: Record<string, number | string>[]
    models: string[]
  }) {
    const top = models.slice(0, 5)
    const config: ChartConfig = Object.fromEntries(
      top.map((model, index) => [
        model.replace(/[^a-z0-9]/gi, '_'),
        { label: model, color: SERIES[index] },
      ]),
    )
    const topModels = new Set(top)
    const data = rows.map((row) => {
      const out: Record<string, number | string> = { day: row.day }
      let other = 0
      for (const model of models) {
        if (topModels.has(model)) out[model.replace(/[^a-z0-9]/gi, '_')] = Number(row[model] ?? 0)
        else other += Number(row[model] ?? 0)
      }
      if (models.length > 5) out.other = other
      return out
    })
    if (models.length > 5) config.other = { label: 'Other', color: 'var(--chart-other)' }
    return (
      <ChartContainer config={config} className="aspect-auto h-64 w-full">
        <BarChart data={data} margin={{ left: 4, right: 4 }}>
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
            width={52}
            tickFormatter={(value: number) => `$${formatCount(value)}`}
          />
          <ChartTooltip
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) =>
                  `Week of ${formatDay(String(payload?.[0]?.payload?.day ?? ''))}`
                }
                formatter={(value, name) => {
                  const label = config[String(name)]?.label
                  return tooltipRow(
                    typeof label === 'string' ? label : String(name),
                    formatUsdTotal(Number(value)),
                  )
                }}
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          {Object.keys(config).map((key, index, keys) => (
            <Bar
              key={key}
              dataKey={key}
              stackId="cost"
              fill={`var(--color-${key})`}
              radius={index === keys.length - 1 ? [3, 3, 0, 0] : 0}
              {...chartAnimation}
            />
          ))}
        </BarChart>
      </ChartContainer>
    )
  }
  return { default: ModelMixChart }
})

export function ModelMixChart(props: ComponentProps<typeof LazyModelMixChart>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazyModelMixChart {...props} />
    </Suspense>
  )
}

const SURVIVAL_CONFIG = {
  ai: { label: 'AI lines', color: AI_TONE },
  human: { label: 'Human lines', color: HUMAN_TONE },
} satisfies ChartConfig

/** Share of merged lines still unchanged N days after merge. */
const LazySurvivalChart = lazy(async () => {
  const [
    { CartesianGrid, Line, LineChart, ReferenceLine, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()
  function SurvivalChart({ rows }: { rows: { day: number; ai: number; human: number }[] }) {
    return (
      <ChartContainer config={SURVIVAL_CONFIG} className="aspect-auto h-64 w-full">
        <LineChart data={rows} margin={{ left: 4, right: 12, top: 8 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="day"
            type="number"
            domain={[0, 90]}
            ticks={[0, 7, 14, 30, 60, 90]}
            tickLine={false}
            axisLine={false}
            tickFormatter={(value: number) => `${value} d`}
          />
          <YAxis
            domain={[0.4, 1]}
            tickLine={false}
            axisLine={false}
            width={40}
            tickFormatter={(value: number) => PERCENT.format(value)}
          />
          <ReferenceLine x={30} stroke="var(--border)" strokeDasharray="4 3" />
          <ChartTooltip
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) =>
                  `Day ${payload?.[0]?.payload?.day ?? ''} after merge`
                }
                formatter={(value, name) =>
                  tooltipRow(
                    SURVIVAL_CONFIG[name as keyof typeof SURVIVAL_CONFIG]?.label ?? String(name),
                    PERCENT.format(Number(value)),
                  )
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          <Line
            dataKey="human"
            type="monotone"
            stroke="var(--color-human)"
            strokeWidth={2}
            dot={false}
            {...chartAnimation}
          />
          <Line
            dataKey="ai"
            type="monotone"
            stroke="var(--color-ai)"
            strokeWidth={2}
            dot={false}
            {...chartAnimation}
          />
        </LineChart>
      </ChartContainer>
    )
  }
  return { default: SurvivalChart }
})

export function SurvivalChart(props: ComponentProps<typeof LazySurvivalChart>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazySurvivalChart {...props} />
    </Suspense>
  )
}

const HISTOGRAM_CONFIG = {
  count: { label: 'Pull requests', color: 'var(--chart-hero)' },
} satisfies ChartConfig

/** A distribution as bars, with the median marked. */
const LazyHistogram = lazy(async () => {
  const [
    { Bar, BarChart, CartesianGrid, ReferenceLine, XAxis, YAxis },
    { ChartContainer, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()
  function Histogram({
    bins,
    medianLabel,
  }: {
    bins: { label: string; count: number; median?: boolean }[]
    medianLabel?: string
  }) {
    const medianBin = bins.find((bin) => bin.median)?.label
    return (
      <ChartContainer config={HISTOGRAM_CONFIG} className="aspect-auto h-52 w-full">
        <BarChart data={bins} margin={{ left: 4, right: 4, top: 16 }}>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="label"
            tickLine={false}
            axisLine={false}
            interval={0}
            tick={{ fontSize: 11 }}
          />
          <YAxis allowDecimals={false} tickLine={false} axisLine={false} width={32} />
          {medianBin ? (
            <ReferenceLine
              x={medianBin}
              stroke="var(--foreground)"
              strokeDasharray="3 3"
              label={{
                value: medianLabel ?? 'Median',
                position: 'top',
                fill: 'var(--muted-foreground)',
                fontSize: 11,
              }}
            />
          ) : null}
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                hideIndicator
                formatter={(value) => tooltipRow('Pull requests', formatCount(Number(value)))}
              />
            }
          />
          <Bar
            dataKey="count"
            fill="var(--color-count)"
            radius={[3, 3, 0, 0]}
            {...chartAnimation}
          />
        </BarChart>
      </ChartContainer>
    )
  }
  return { default: Histogram }
})

export function Histogram(props: ComponentProps<typeof LazyHistogram>) {
  return (
    <Suspense fallback={<ChartSkeleton />}>
      <LazyHistogram {...props} />
    </Suspense>
  )
}
