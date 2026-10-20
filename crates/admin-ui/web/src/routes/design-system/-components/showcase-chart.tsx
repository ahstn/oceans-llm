import { lazy, Suspense } from 'react'

import type { ChartConfig } from '@/components/ui/chart'
import { Skeleton } from '@/components/ui/skeleton'
import { chartAnimation } from '@/lib/chart-motion'

// Recharts stays out of the route chunk, matching the profile charts.
const loadChartKit = () => Promise.all([import('recharts'), import('@/components/ui/chart')])

const series = ['claude', 'gpt', 'gemini', 'other'] as const

const config = {
  claude: { label: 'Claude', color: 'var(--chart-1)' },
  gpt: { label: 'GPT', color: 'var(--chart-2)' },
  gemini: { label: 'Gemini', color: 'var(--chart-3)' },
  other: { label: 'Other', color: 'var(--chart-other)' },
} satisfies ChartConfig

const rows = [
  { day: 'Mon', claude: 420, gpt: 260, gemini: 120, other: 40 },
  { day: 'Tue', claude: 510, gpt: 240, gemini: 150, other: 60 },
  { day: 'Wed', claude: 480, gpt: 300, gemini: 170, other: 50 },
  { day: 'Thu', claude: 620, gpt: 280, gemini: 140, other: 70 },
  { day: 'Fri', claude: 590, gpt: 330, gemini: 190, other: 80 },
  { day: 'Sat', claude: 210, gpt: 120, gemini: 60, other: 20 },
  { day: 'Sun', claude: 180, gpt: 90, gemini: 50, other: 20 },
]

const LazyRequestsChart = lazy(async () => {
  const [
    { Bar, BarChart, CartesianGrid, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()

  function RequestsChart() {
    return (
      <ChartContainer config={config} className="h-64 w-full">
        <BarChart accessibilityLayer data={rows} margin={{ left: 4, right: 12 }}>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="day" tickLine={false} axisLine={false} />
          <YAxis tickLine={false} axisLine={false} width={40} />
          <ChartTooltip cursor={false} content={<ChartTooltipContent indicator="dot" />} />
          <ChartLegend content={<ChartLegendContent />} />
          {series.map((key, index) => (
            <Bar
              {...chartAnimation}
              key={key}
              dataKey={key}
              stackId="requests"
              fill={`var(--color-${key})`}
              radius={index === series.length - 1 ? [4, 4, 0, 0] : 0}
            />
          ))}
        </BarChart>
      </ChartContainer>
    )
  }

  return { default: RequestsChart }
})

/** Requests per day stacked by model family, drawn with the chart ramp. */
export function ShowcaseChart() {
  return (
    <Suspense fallback={<Skeleton className="h-64 w-full" />}>
      <LazyRequestsChart />
    </Suspense>
  )
}
