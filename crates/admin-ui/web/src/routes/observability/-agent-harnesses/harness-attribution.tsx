// Harness attribution on the Agent Harnesses page: what each harness and model turns spend into.
// It reads the attribution report's harness × model rows: sessions joined to the commits they
// wrote, with each session's gateway cost and whether its work merged or reached the default branch.

import { useState, useTransition } from 'react'
import { toast } from 'sonner'

import { MatrixHeatmap } from '@/components/attribution/heatmaps'
import {
  InlineMeter,
  Insight,
  InsightCard,
  ReadinessBadge,
  ToolLabel,
} from '@/components/attribution/parts'
import { canonicalHarness, formatPercent, harnessLabel } from '@/components/attribution/format'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { getErrorMessage } from '@/lib/errors'
import { EMPTY_VALUE, formatCount, formatUsd10000 } from '@/lib/format'
import { getAttribution } from '@/server/admin-data.functions'
import type { AttributionReportView } from '@/types/api'

type HarnessModelRow = AttributionReportView['harness_models'][number]
type Metric = 'cost' | 'shipped' | 'steering'
type Window = 7 | 30 | 90

type Cell = {
  harness: string
  model: string
  sessions: number
  shipped: number
  aiLines: number
  /** Sessions with requests, all of them priced: the only population cost per line uses. */
  pricedSessions: number
  pricedLines: number
  pricedCostUsd10000: number
}

function merge(rows: HarnessModelRow[]) {
  const cells = new Map<string, Cell>()
  for (const row of rows) {
    const harness = canonicalHarness(row.harness)
    const key = `${harness}::${row.model_key}`
    const cell = cells.get(key) ?? {
      harness,
      model: row.model_key,
      sessions: 0,
      shipped: 0,
      aiLines: 0,
      pricedSessions: 0,
      pricedLines: 0,
      pricedCostUsd10000: 0,
    }
    cell.sessions += row.session_count
    cell.shipped += row.shipped_session_count
    cell.aiLines += row.ai_lines
    cell.pricedSessions += row.fully_priced_session_count
    cell.pricedLines += row.fully_priced_ai_lines
    cell.pricedCostUsd10000 += row.fully_priced_cost_usd_10000
    cells.set(key, cell)
  }
  return [...cells.values()]
}

/**
 * Cost per 100 committed AI lines over fully priced sessions only, so a session the gateway could
 * not join or price never reads as free output. `null` without such sessions or their lines.
 */
function costPer100(cell: Pick<Cell, 'pricedSessions' | 'pricedLines' | 'pricedCostUsd10000'>) {
  return cell.pricedSessions > 0 && cell.pricedLines > 0
    ? (cell.pricedCostUsd10000 / cell.pricedLines) * 100
    : null
}

export function HarnessAttribution({
  initial,
  initialFailed = false,
}: {
  initial: AttributionReportView | null
  /** The page loader could not read the report, which is not the same as having no sessions. */
  initialFailed?: boolean
}) {
  const [report, setReport] = useState(initial)
  const [failed, setFailed] = useState(initialFailed)
  const [windowDays, setWindowDays] = useState<Window>((initial?.window_days as Window) ?? 7)
  const [isPending, startTransition] = useTransition()

  function changeWindow(next: Window) {
    startTransition(async () => {
      try {
        const response = await getAttribution({ data: { days: next, compare: false } })
        // The toggle moves only with the report, so a failed load keeps both on the old window.
        setReport(response.data)
        setWindowDays(next)
        setFailed(false)
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  const cells = merge(report?.harness_models ?? [])
  const coverage = report?.coverage
  const incomplete = Boolean(
    coverage &&
    (coverage.commits_truncated ||
      coverage.pull_requests_truncated ||
      coverage.agent_sessions_truncated ||
      coverage.request_facts_truncated),
  )
  const totals = harnessTotals(cells)
  const harnesses = totals.map((row) => row.harness)
  const models = [...new Set(cells.map((cell) => cell.model))].sort()
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

      {failed && !report ? (
        <p className="text-muted-foreground rounded-xl border p-6 text-center text-sm">
          Attribution could not be loaded. Choose a window to try again.
        </p>
      ) : totals.length === 0 ? (
        <p className="text-muted-foreground rounded-xl border p-6 text-center text-sm">
          No attributed agent sessions in this window. Sessions appear once{' '}
          <code className="font-mono">oceans hook install --git-ai</code> reports commits.
        </p>
      ) : (
        <>
          {incomplete ? (
            <Alert>
              <AlertTitle>This comparison is incomplete</AlertTitle>
              <AlertDescription>
                The window holds more commits, sessions or requests than one report reads, so costs
                can read low and sessions can look unshipped. The cheapest-pair callout is hidden.
                Choose a shorter window.
              </AlertDescription>
            </Alert>
          ) : null}
          <HarnessTiles totals={totals} />
          <HarnessModelCard
            cells={cells}
            harnesses={harnesses}
            models={models}
            incomplete={incomplete}
          />
        </>
      )}
    </section>
  )
}

type HarnessTotal = Omit<Cell, 'model'>

/** One total per harness, summed in a single pass over the cells, most sessions first. */
function harnessTotals(cells: Cell[]): HarnessTotal[] {
  const totals = new Map<string, HarnessTotal>()
  for (const { model: _model, ...cell } of cells) {
    const total = totals.get(cell.harness)
    if (!total) {
      totals.set(cell.harness, { ...cell })
      continue
    }
    total.sessions += cell.sessions
    total.shipped += cell.shipped
    total.aiLines += cell.aiLines
    total.pricedSessions += cell.pricedSessions
    total.pricedLines += cell.pricedLines
    total.pricedCostUsd10000 += cell.pricedCostUsd10000
  }
  return [...totals.values()].sort((a, b) => b.sessions - a.sessions)
}

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
  incomplete,
}: {
  cells: Cell[]
  harnesses: string[]
  models: string[]
  /** A read cap cut the report short, so no pair is called out as cheapest. */
  incomplete: boolean
}) {
  const [metric, setMetric] = useState<Metric>('cost')
  const ranked = cells
    .filter((cell) => cell.pricedSessions >= 3 && costPer100(cell) != null)
    .sort((a, b) => (costPer100(a) ?? 0) - (costPer100(b) ?? 0))
  // The heatmap reads every cell twice (range, then render), so look each up in constant time.
  const byPair = new Map(cells.map((entry) => [`${entry.harness}::${entry.model}`, entry]))
  const cell = (harness: string, model: string) => byPair.get(`${harness}::${model}`)
  return (
    <InsightCard
      title="Harness by model"
      description={
        metric === 'cost'
          ? 'Gateway cost per 100 committed AI lines, from sessions whose requests were all priced. Brightest is cheapest; blank pairs have no such sessions.'
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
        {metric !== 'steering' && !incomplete && ranked.length > 1 ? (
          <Insight>
            {ranked[0].model} in {harnessLabel(ranked[0].harness)} lands committed lines at{' '}
            {formatUsd10000(costPer100(ranked[0]) ?? 0)} per 100, against{' '}
            {formatUsd10000(costPer100(ranked[ranked.length - 1]) ?? 0)} for{' '}
            {ranked[ranked.length - 1].model} in {harnessLabel(ranked[ranked.length - 1].harness)}.
            Check the Shipped view before switching: cheap lines that never merge cost more. Pairs
            with fewer than 3 fully priced sessions are left out of this comparison.
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
