// Harness attribution on the Agent Harnesses page: what each harness and model turns spend into.
// It reads the attribution report's harness × model rows: sessions joined to the commits they
// wrote, with each session's gateway cost and whether its work merged or reached the default branch.

import { useState, useTransition } from 'react'
import { toast } from 'sonner'

import { MatrixHeatmap } from '@/components/attribution/heatmaps'
import {
  formatPercent,
  InlineMeter,
  Insight,
  InsightCard,
  ReadinessBadge,
  ToolLabel,
  harnessLabel,
} from '@/components/attribution/parts'
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { getErrorMessage } from '@/lib/errors'
import { EMPTY_VALUE, formatCount, formatUsd10000 } from '@/lib/format'
import { getAttribution } from '@/server/admin-data.functions'
import type { AttributionReportView } from '@/types/api'

type HarnessModelRow = AttributionReportView['harness_models'][number]
type Metric = 'cost' | 'shipped' | 'steering'
type Window = 7 | 30 | 90

/** git-ai's `claude` and the Claude Code hook's `claude_code` are one harness. */
const HARNESS_ALIASES: Record<string, string> = { claude_code: 'claude' }

type Cell = {
  harness: string
  model: string
  sessions: number
  shipped: number
  aiLines: number
  costUsd10000: number
  pricedRequests: number
}

function merge(rows: HarnessModelRow[]) {
  const cells = new Map<string, Cell>()
  for (const row of rows) {
    const harness = HARNESS_ALIASES[row.harness] ?? row.harness
    const key = `${harness}::${row.model_key}`
    const cell = cells.get(key) ?? {
      harness,
      model: row.model_key,
      sessions: 0,
      shipped: 0,
      aiLines: 0,
      costUsd10000: 0,
      pricedRequests: 0,
    }
    cell.sessions += row.session_count
    cell.shipped += row.shipped_session_count
    cell.aiLines += row.ai_lines
    cell.costUsd10000 += row.priced_cost_usd_10000
    cell.pricedRequests += row.priced_request_count
    cells.set(key, cell)
  }
  return [...cells.values()]
}

/** Priced cost per 100 committed AI lines, or `null` without lines or prices. */
function costPer100(cell: Pick<Cell, 'aiLines' | 'costUsd10000' | 'pricedRequests'>) {
  return cell.aiLines > 0 && cell.pricedRequests > 0
    ? (cell.costUsd10000 / cell.aiLines) * 100
    : null
}

export function HarnessAttribution({ initial }: { initial: AttributionReportView | null }) {
  const [report, setReport] = useState(initial)
  const [windowDays, setWindowDays] = useState<Window>((initial?.window_days as Window) ?? 7)
  const [isPending, startTransition] = useTransition()

  function changeWindow(next: Window) {
    setWindowDays(next)
    startTransition(async () => {
      try {
        const response = await getAttribution({ data: { days: next } })
        setReport(response.data)
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  const cells = merge(report?.harness_models ?? [])
  const harnesses = [...new Set(cells.map((cell) => cell.harness))].sort(
    (a, b) =>
      cells.filter((cell) => cell.harness === b).reduce((t, c) => t + c.sessions, 0) -
      cells.filter((cell) => cell.harness === a).reduce((t, c) => t + c.sessions, 0),
  )
  const models = [...new Set(cells.map((cell) => cell.model))].sort()
  const totals = harnesses.map((harness) => {
    const own = cells.filter((cell) => cell.harness === harness)
    const sum = (pick: (cell: Cell) => number) => own.reduce((total, cell) => total + pick(cell), 0)
    return {
      harness,
      sessions: sum((cell) => cell.sessions),
      shipped: sum((cell) => cell.shipped),
      aiLines: sum((cell) => cell.aiLines),
      costUsd10000: sum((cell) => cell.costUsd10000),
      pricedRequests: sum((cell) => cell.pricedRequests),
    }
  })
  return (
    <section
      aria-label="Harness attribution"
      aria-busy={isPending}
      className={
        isPending ? 'flex flex-col gap-6 opacity-60 transition-opacity' : 'flex flex-col gap-6'
      }
    >
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-lg font-medium">What each harness ships</h2>
          <p className="text-muted-foreground text-sm">
            Agent sessions joined to the commits they wrote, from Git AI attribution.
          </p>
        </div>
        <ToggleGroup
          type="single"
          variant="outline"
          value={String(windowDays)}
          onValueChange={(next) => next && changeWindow(Number(next) as Window)}
          disabled={isPending}
          aria-label="Attribution window"
        >
          {[7, 30, 90].map((days) => (
            <ToggleGroupItem
              key={days}
              value={String(days)}
              aria-label={`Attribution for the last ${days} days`}
            >
              {days}d
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      {totals.length === 0 ? (
        <p className="text-muted-foreground rounded-xl border p-6 text-center text-sm">
          No attributed agent sessions in this window. Sessions appear once{' '}
          <code className="font-mono">oceans hook install --git-ai</code> reports commits.
        </p>
      ) : (
        <>
          <HarnessTiles totals={totals} />
          <HarnessModelCard cells={cells} harnesses={harnesses} models={models} />
        </>
      )}
    </section>
  )
}

type HarnessTotal = Omit<Cell, 'model'>

function HarnessTiles({ totals }: { totals: HarnessTotal[] }) {
  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
      {totals.map((row) => {
        const cost = costPer100(row)
        return (
          <Card key={row.harness} size="sm" className="min-w-0">
            <CardHeader>
              <CardDescription className="flex items-center justify-between gap-2">
                <ToolLabel harness={row.harness} />
                <span className="text-xs tabular-nums">{formatCount(row.sessions)} sessions</span>
              </CardDescription>
              <p className="flex items-baseline gap-1.5 text-2xl font-medium tracking-tight tabular-nums">
                {cost == null ? EMPTY_VALUE : formatUsd10000(cost)}
                <span className="text-muted-foreground text-xs font-normal">per 100 AI lines</span>
              </p>
            </CardHeader>
            <CardContent className="flex flex-col gap-2 text-xs">
              <div className="flex flex-col gap-1">
                <span className="text-muted-foreground">Sessions that shipped</span>
                <InlineMeter
                  value={row.sessions > 0 ? row.shipped / row.sessions : null}
                  tone="var(--chart-4)"
                />
              </div>
              <div className="text-muted-foreground flex items-center justify-between gap-2">
                <span>Steering / session</span>
                <ReadinessBadge
                  level="new"
                  note="Needs user turns per session, counted from requests the gateway already logs."
                />
              </div>
            </CardContent>
          </Card>
        )
      })}
    </div>
  )
}

function HarnessModelCard({
  cells,
  harnesses,
  models,
}: {
  cells: Cell[]
  harnesses: string[]
  models: string[]
}) {
  const [metric, setMetric] = useState<Metric>('cost')
  const ranked = cells
    .filter((cell) => cell.sessions >= 3 && costPer100(cell) != null)
    .sort((a, b) => (costPer100(a) ?? 0) - (costPer100(b) ?? 0))
  const cell = (harness: string, model: string) =>
    cells.find((entry) => entry.harness === harness && entry.model === model)
  return (
    <InsightCard
      title="Harness by model"
      description={
        metric === 'cost'
          ? 'Gateway cost per 100 committed AI lines. Brightest is cheapest; blank pairs were not used.'
          : metric === 'shipped'
            ? 'Share of sessions whose work merged or reached the default branch. Brightest ships most.'
            : 'Human turns that redirected the agent, per session. Oceans does not count these yet.'
      }
      readiness={metric === 'steering' ? 'new' : undefined}
      readinessNote="Needs user turns per session, counted from requests the gateway already logs."
      action={
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={metric}
          onValueChange={(next) => next && setMetric(next as Metric)}
          aria-label="Harness by model metric"
        >
          <ToggleGroupItem value="cost">Cost per line</ToggleGroupItem>
          <ToggleGroupItem value="shipped">Shipped</ToggleGroupItem>
          <ToggleGroupItem value="steering">Steering</ToggleGroupItem>
        </ToggleGroup>
      }
    >
      <div className="flex flex-col gap-4">
        <MatrixHeatmap
          caption="Harness by model"
          rows={harnesses}
          columns={models}
          rowLabel={(harness) => <ToolLabel harness={harness} />}
          columnLabel={(model) => model}
          invert={metric === 'cost'}
          value={(harness, model) => {
            const entry = cell(harness, model)
            if (!entry || metric === 'steering') return null
            if (metric === 'cost') return costPer100(entry)
            return entry.sessions > 0 ? entry.shipped / entry.sessions : null
          }}
          format={(value) => (metric === 'cost' ? formatUsd10000(value) : formatPercent(value))}
        />
        {metric !== 'steering' && ranked.length > 1 ? (
          <Insight>
            {ranked[0].model} in {harnessLabel(ranked[0].harness)} lands committed lines at{' '}
            {formatUsd10000(costPer100(ranked[0]) ?? 0)} per 100, against{' '}
            {formatUsd10000(costPer100(ranked[ranked.length - 1]) ?? 0)} for{' '}
            {ranked[ranked.length - 1].model} in {harnessLabel(ranked[ranked.length - 1].harness)}.
            Check the Shipped view before switching: cheap lines that never merge cost more. Pairs
            with fewer than 3 sessions are left out of this comparison.
          </Insight>
        ) : null}
        {metric === 'steering' ? (
          <p className="text-muted-foreground text-xs">
            Steering counts the human messages that redirect an agent mid-session. The gateway logs
            those messages but does not count them per session yet.
          </p>
        ) : null}
      </div>
    </InsightCard>
  )
}
