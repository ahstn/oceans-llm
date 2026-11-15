// Concept 5, Repositories: where agents work well and where the harness needs investment. A
// quadrant plots AI share against how long AI code lasts; the table beside it gives each repository
// its autonomy, thrash ratio, cost and durability so the worst rows point at the next fix.

import { useMemo, useState, type CSSProperties } from 'react'

import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatCount, formatUsd10000 } from '@/lib/format'
import { cn } from '@/lib/utils'

import { getMockDataset, shortRepository } from './fixtures'
import { repositoryRows, sliceWindow, type ConceptWindow, type RepositoryRow } from './metrics'
import {
  AI_TONE,
  InsightCard,
  DECIMAL_1,
  formatHours,
  formatPercent,
  InlineMeter,
  Insight,
  ReadinessBadge,
  Sparkline,
} from '@/components/attribution/parts'

export default function RepositoriesConcept({ days }: { days: ConceptWindow }) {
  const rows = useMemo(() => repositoryRows(sliceWindow(getMockDataset(), days)), [days])
  const [focus, setFocus] = useState<string | null>(null)
  const worst = [...rows].sort(
    (a, b) => (b.generatedPerCommitted ?? 0) - (a.generatedPerCommitted ?? 0),
  )[0]

  return (
    <div className="flex flex-col gap-6">
      <InsightCard
        title="Agent readiness"
        description="AI share across, 30-day durability up, bubble size by spend. Top right is leverage; bottom right is AI code that does not last. Invest in the repositories with the lowest autonomy and the worst generated-to-committed ratio."
        readiness="new"
        readinessNote="AI share and autonomy are derivable; durability and generated lines need new data."
      >
        <div className="grid gap-8 lg:grid-cols-[minmax(0,24rem)_minmax(0,1fr)]">
          <Quadrant rows={rows} focus={focus} onFocus={setFocus} />
          <CompareList rows={rows} focus={focus} onFocus={setFocus} />
        </div>
      </InsightCard>

      <InsightCard
        title="Repositories"
        description="Sorted by commits. Read across a row: high AI share with high thrash and low durability means agents lack context there."
        readiness="derived"
        readinessNote="Group live commit and PR rows by repository; durability needs new data."
      >
        <div className="flex flex-col gap-4">
          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Repositories" className="min-w-[56rem]">
              <AdminTableHeader
                columns={[
                  'Repository',
                  'AI share, 13 weeks',
                  'Autonomy',
                  { label: 'Generated : committed', align: 'right' },
                  { label: 'Cost per PR', align: 'right' },
                  { label: 'Lead time', align: 'right' },
                  '30-day durability',
                ]}
              />
              <TableBody>
                {rows.map((row) => (
                  <TableRow
                    key={row.repository}
                    data-state={focus === row.repository ? 'selected' : undefined}
                    onMouseEnter={() => setFocus(row.repository)}
                    onMouseLeave={() => setFocus(null)}
                  >
                    <TableCell className="px-3 py-2">
                      <p className="font-mono text-sm">{shortRepository(row.repository)}</p>
                      <p className="text-muted-foreground text-xs">
                        {formatCount(row.commits)} commits · {formatCount(row.mergedPrs)} merged
                      </p>
                    </TableCell>
                    <TableCell className="w-44 px-3 py-2">
                      <div className="flex items-center gap-2">
                        <Sparkline
                          values={row.weeklyAiShare}
                          tone={AI_TONE}
                          height={24}
                          className="w-24"
                        />
                        <span className="text-xs tabular-nums">{formatPercent(row.aiShare)}</span>
                      </div>
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <InlineMeter value={row.autonomy} tone="var(--chart-3)" />
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {row.generatedPerCommitted == null
                        ? '—'
                        : `${DECIMAL_1.format(row.generatedPerCommitted)} : 1`}
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {row.medianCostPerMergedPr == null
                        ? '—'
                        : formatUsd10000(row.medianCostPerMergedPr)}
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {formatHours(row.medianLeadTimeMs)}
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <InlineMeter value={row.durable30} tone="var(--chart-2)" />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
          <div className="text-muted-foreground flex flex-wrap gap-x-5 gap-y-2 text-xs">
            <span className="inline-flex items-center gap-1.5">
              Autonomy: AI lines committed with no human edit in the same commit{' '}
              <ReadinessBadge level="derived" />
            </span>
            <span className="inline-flex items-center gap-1.5">
              Generated : committed{' '}
              <ReadinessBadge level="new" note="Needs git-ai checkpoint line counts." />
            </span>
          </div>
          {worst ? (
            <Insight>
              <span className="font-mono">{shortRepository(worst.repository)}</span> has agents
              writing {DECIMAL_1.format(worst.generatedPerCommitted ?? 0)} lines per committed line.
              An AGENTS.md, test fixtures or a skill there is likely to cut its spend more than a
              model change.
            </Insight>
          ) : null}
        </div>
      </InsightCard>
    </div>
  )
}

/** An SVG quadrant: AI share (x) by durability (y), bubbles sized by spend. */
// Axes, quadrant labels and bubbles share one coordinate system, so they stay in one SVG.
// oxlint-disable-next-line eslint/max-lines-per-function
function Quadrant({
  rows,
  focus,
  onFocus,
}: {
  rows: RepositoryRow[]
  focus: string | null
  onFocus: (repository: string | null) => void
}) {
  const size = 360
  const pad = 36
  const maxCost = Math.max(1, ...rows.map((row) => row.costUsd10000))
  // Axes zoom to the data; the dividers sit at the team average, so quadrants read as
  // above or below the team rather than against arbitrary targets.
  const plotted = rows.filter((row) => row.aiShare != null && row.durable30 != null)
  const domain = (values: number[]) => [
    Math.max(0, Math.floor((Math.min(...values) - 0.06) * 10) / 10),
    Math.min(1, Math.ceil((Math.max(...values) + 0.06) * 10) / 10),
  ]
  const [x0, x1] = domain(plotted.map((row) => row.aiShare ?? 0))
  const [y0, y1] = domain(plotted.map((row) => row.durable30 ?? 0))
  const weight = Math.max(
    1,
    plotted.reduce((total, row) => total + row.commits, 0),
  )
  const meanX = plotted.reduce((total, row) => total + (row.aiShare ?? 0) * row.commits, 0) / weight
  const meanY =
    plotted.reduce((total, row) => total + (row.durable30 ?? 0) * row.commits, 0) / weight
  const x = (value: number) => pad + ((value - x0) / (x1 - x0)) * (size - pad * 2)
  const y = (value: number) => size - pad - ((value - y0) / (y1 - y0)) * (size - pad * 2)
  const quadrants = [
    { label: 'Untapped', x: pad + 6, y: pad + 14 },
    { label: 'Leverage', x: size - pad - 6, y: pad + 14, end: true },
    { label: 'Needs investment', x: pad + 6, y: size - pad - 8 },
    { label: 'Churn risk', x: size - pad - 6, y: size - pad - 8, end: true },
  ]
  return (
    <div className="flex flex-col gap-3">
      <svg
        viewBox={`0 0 ${size} ${size}`}
        className="h-auto w-full"
        role="img"
        aria-label="Repositories by AI share and durability"
      >
        <rect
          x={pad}
          y={pad}
          width={size - pad * 2}
          height={size - pad * 2}
          className="stroke-border fill-none"
        />
        <line
          x1={x(meanX)}
          x2={x(meanX)}
          y1={pad}
          y2={size - pad}
          className="stroke-border"
          strokeDasharray="4 3"
        />
        <line
          x1={pad}
          x2={size - pad}
          y1={y(meanY)}
          y2={y(meanY)}
          className="stroke-border"
          strokeDasharray="4 3"
        />
        {quadrants.map((quadrant) => (
          <text
            key={quadrant.label}
            x={quadrant.x}
            y={quadrant.y}
            textAnchor={quadrant.end ? 'end' : 'start'}
            className="fill-muted-foreground text-2xs"
          >
            {quadrant.label}
          </text>
        ))}
        {[x0, meanX, x1].map((tick) => (
          <text
            key={tick}
            x={x(tick)}
            y={size - pad + 16}
            textAnchor="middle"
            className="fill-muted-foreground text-2xs"
          >
            {formatPercent(tick)}
          </text>
        ))}
        {[y0, meanY, y1].map((tick) => (
          <text
            key={tick}
            x={pad - 6}
            y={y(tick) + 3}
            textAnchor="end"
            className="fill-muted-foreground text-2xs"
          >
            {formatPercent(tick)}
          </text>
        ))}
        <text
          x={size / 2}
          y={size - 4}
          textAnchor="middle"
          className="fill-muted-foreground text-2xs"
        >
          AI share of added lines
        </text>
        {rows.map((row, index) => {
          if (row.aiShare == null || row.durable30 == null) return null
          const radius = 6 + Math.sqrt(row.costUsd10000 / maxCost) * 18
          const active = !focus || focus === row.repository
          return (
            <g
              key={row.repository}
              onMouseEnter={() => onFocus(row.repository)}
              onMouseLeave={() => onFocus(null)}
              className="transition-opacity duration-200"
              style={{ opacity: active ? 1 : 0.3 }}
            >
              <circle
                cx={x(row.aiShare)}
                cy={y(row.durable30)}
                r={radius}
                className="fill-(--bubble)/35 stroke-(--bubble)"
                style={{ '--bubble': `var(--chart-${(index % 5) + 1})` } as CSSProperties}
              >
                <title>{`${shortRepository(row.repository)}: ${formatPercent(row.aiShare)} AI, ${formatPercent(row.durable30)} durable, ${formatUsd10000(row.costUsd10000)}`}</title>
              </circle>
              <text
                x={
                  x(row.aiShare) > size * 0.62
                    ? x(row.aiShare) - radius - 4
                    : x(row.aiShare) + radius + 4
                }
                y={y(row.durable30) + 3}
                textAnchor={x(row.aiShare) > size * 0.62 ? 'end' : 'start'}
                className={cn(
                  'text-2xs',
                  focus === row.repository ? 'fill-foreground' : 'fill-muted-foreground',
                )}
              >
                {shortRepository(row.repository).split('/')[1]}
              </text>
            </g>
          )
        })}
      </svg>
      <p className="text-muted-foreground text-xs">
        Bubble area is gateway spend in the window. Dashed lines mark the team average for each
        axis.
      </p>
    </div>
  )
}

/** Git AI's repo comparison: two meters per repository, worst generated : committed in amber. */
function CompareList({
  rows,
  focus,
  onFocus,
}: {
  rows: RepositoryRow[]
  focus: string | null
  onFocus: (repository: string | null) => void
}) {
  const worstRatio = Math.max(...rows.map((row) => row.generatedPerCommitted ?? 0))
  return (
    <ul className="divide-border flex flex-col divide-y">
      {rows.map((row) => (
        <li
          key={row.repository}
          className={cn(
            'grid gap-x-6 gap-y-2 py-3 transition-opacity duration-200 sm:grid-cols-[10rem_minmax(0,1fr)_minmax(0,1fr)_6rem]',
            focus && focus !== row.repository && 'opacity-40',
          )}
          onMouseEnter={() => onFocus(row.repository)}
          onMouseLeave={() => onFocus(null)}
        >
          <span className="self-center truncate font-mono text-sm">
            {shortRepository(row.repository)}
          </span>
          <div className="flex flex-col gap-1">
            <span className="text-muted-foreground text-xs">Autonomy</span>
            <InlineMeter value={row.autonomy} tone="var(--chart-3)" />
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-muted-foreground text-xs">Durable at 30 days</span>
            <InlineMeter value={row.durable30} tone="var(--chart-2)" />
          </div>
          <div className="flex flex-col items-end gap-1">
            <span className="text-muted-foreground text-xs">Gen : commit</span>
            <span
              className={cn(
                'text-sm font-medium tabular-nums',
                row.generatedPerCommitted === worstRatio && 'text-warning',
              )}
            >
              {row.generatedPerCommitted == null
                ? '—'
                : `${DECIMAL_1.format(row.generatedPerCommitted)} : 1`}
            </span>
          </div>
        </li>
      ))}
    </ul>
  )
}
