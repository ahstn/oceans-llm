import { lazy } from 'react'

import { formatDay, PERCENT_FORMATTER } from './shared'
import { COMPACT_FORMATTER, type SeriesChart } from './token-series'

// Recharts stays out of the route chunk; each chart loads on first render like the spend trend.
const loadChartKit = () => Promise.all([import('recharts'), import('@/components/ui/chart')])

function seriesTooltipRow(chart: SeriesChart, name: unknown, value: string) {
  return (
    <div className="flex w-full items-center justify-between gap-4">
      <span className="text-muted-foreground">
        {chart.config[String(name)]?.label ?? String(name)}
      </span>
      <span className="font-mono font-medium tabular-nums">{value}</span>
    </div>
  )
}

/** Each day's token mix across models, normalised to 100%. */
export const ModelShareBarChart = lazy(async () => {
  const [
    { Bar, BarChart, CartesianGrid, XAxis, YAxis },
    { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent },
  ] = await loadChartKit()

  function ModelShareBarChartComponent({ chart }: { chart: SeriesChart }) {
    return (
      <ChartContainer config={chart.config} className="h-72 w-full">
        <BarChart
          accessibilityLayer
          data={chart.rows}
          stackOffset="expand"
          margin={{ left: 4, right: 12 }}
        >
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
            width={48}
            tickFormatter={(value: number) => PERCENT_FORMATTER.format(value)}
          />
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                labelFormatter={(_, payload) => formatDay(String(payload?.[0]?.payload?.day ?? ''))}
                formatter={(value, name) =>
                  seriesTooltipRow(chart, name, COMPACT_FORMATTER.format(Number(value)))
                }
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          {chart.keys.map((series) => (
            <Bar key={series.key} dataKey={series.key} stackId="share" fill={series.color} />
          ))}
        </BarChart>
      </ChartContainer>
    )
  }

  return { default: ModelShareBarChartComponent }
})
