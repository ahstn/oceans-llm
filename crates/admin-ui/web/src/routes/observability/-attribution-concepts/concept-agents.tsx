// Concept 6, Agents and models: which harness and model pairs turn spend into shipped code. A tile
// per harness, a harness by model matrix of cost per 100 committed AI lines, and the weekly model mix,
// so a model switch can be judged on outcome rather than price per token.

import { useMemo, useState } from 'react'

import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { formatCount, formatUsd10000 } from '@/lib/format'

import { ModelMixChart } from './charts'
import { AGENT_TOOLS, getMockDataset, type AgentTool } from './fixtures'
import { MatrixHeatmap } from './heatmaps'
import {
  sliceWindow,
  toolModelCells,
  toolTotals,
  weeklyModelCost,
  type ConceptWindow,
} from './metrics'
import {
  ConceptCard,
  DECIMAL_1,
  formatPercent,
  InlineMeter,
  Insight,
  ReadinessBadge,
  ToolLabel,
} from './parts'

type MatrixMetric = 'cost' | 'merged' | 'steering'

// A concept page is one linear layout read top to bottom; splitting it would scatter the mockup.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function AgentsConcept({ days }: { days: ConceptWindow }) {
  const [metric, setMetric] = useState<MatrixMetric>('cost')
  const model = useMemo(() => {
    const dataset = getMockDataset()
    const slice = sliceWindow(dataset, days)
    const cells = toolModelCells(dataset, slice)
    return {
      cells,
      tools: toolTotals(dataset, slice),
      mix: weeklyModelCost(sliceWindow(dataset, days < 30 ? 30 : days)),
    }
  }, [days])

  const models = [...new Set(model.cells.map((cell) => cell.model))].sort()
  const tools = AGENT_TOOLS.filter((tool) => model.cells.some((cell) => cell.tool === tool))
  const cell = (tool: AgentTool, name: string) =>
    model.cells.find((entry) => entry.tool === tool && entry.model === name)
  const cheapest = [...model.cells]
    .filter((entry) => entry.costPer100Lines != null && entry.sessions >= 5)
    .sort((a, b) => (a.costPer100Lines ?? 0) - (b.costPer100Lines ?? 0))

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
        {model.tools.map((tool) => (
          <Card key={tool.tool} size="sm" className="min-w-0">
            <CardHeader>
              <CardDescription className="flex items-center justify-between gap-2">
                <ToolLabel tool={tool.tool} />
                <span className="text-xs tabular-nums">{formatCount(tool.sessions)} sessions</span>
              </CardDescription>
              <p className="flex items-baseline gap-1.5 text-2xl font-medium tracking-tight tabular-nums">
                {tool.costPer100Lines == null ? '—' : formatUsd10000(tool.costPer100Lines)}
                <span className="text-muted-foreground text-xs font-normal">per 100 AI lines</span>
              </p>
            </CardHeader>
            <CardContent className="flex flex-col gap-2 text-xs">
              <div className="flex flex-col gap-1">
                <span className="text-muted-foreground">Sessions that shipped</span>
                <InlineMeter value={tool.mergedShare} tone="var(--chart-4)" />
              </div>
              <div className="text-muted-foreground flex items-center justify-between gap-2">
                <span className="inline-flex items-center gap-1.5">
                  Steering / session{' '}
                  <ReadinessBadge
                    level="proxy"
                    note="Count user turns in the session's gateway requests."
                  />
                </span>
                <span className="text-foreground tabular-nums">
                  {DECIMAL_1.format(tool.steeringPerSession ?? 0)}
                </span>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <ConceptCard
        title="Harness by model"
        description={
          metric === 'cost'
            ? 'Gateway cost per 100 committed AI lines. Brightest is cheapest; blank pairs were not used.'
            : metric === 'merged'
              ? 'Share of sessions whose work reached the default branch. Brightest ships most.'
              : 'Human turns that redirected the agent, per session. Brightest needs the least steering.'
        }
        readiness={metric === 'steering' ? 'proxy' : 'derived'}
        readinessNote="Sessions carry tool and model; outcomes come from the session flow."
        action={
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={metric}
            onValueChange={(next) => next && setMetric(next as MatrixMetric)}
            aria-label="Matrix metric"
          >
            <ToggleGroupItem value="cost">Cost per line</ToggleGroupItem>
            <ToggleGroupItem value="merged">Shipped</ToggleGroupItem>
            <ToggleGroupItem value="steering">Steering</ToggleGroupItem>
          </ToggleGroup>
        }
      >
        <div className="flex flex-col gap-4">
          <MatrixHeatmap
            caption="Harness by model"
            rows={tools}
            columns={models}
            rowLabel={(tool) => <ToolLabel tool={tool} />}
            columnLabel={(name) => name}
            invert={metric !== 'merged'}
            value={(tool, name) => {
              const entry = cell(tool, name)
              if (!entry) return null
              if (metric === 'cost') return entry.costPer100Lines
              if (metric === 'merged') return entry.mergedShare
              return entry.steeringPerSession
            }}
            format={(value) =>
              metric === 'cost'
                ? formatUsd10000(value)
                : metric === 'merged'
                  ? formatPercent(value)
                  : DECIMAL_1.format(value)
            }
          />
          {cheapest.length > 1 ? (
            <Insight>
              {cheapest[0].model} in {cheapest[0].tool} lands committed lines at{' '}
              {formatUsd10000(cheapest[0].costPer100Lines ?? 0)} per 100, against{' '}
              {formatUsd10000(cheapest[cheapest.length - 1].costPer100Lines ?? 0)} for{' '}
              {cheapest[cheapest.length - 1].model} in {cheapest[cheapest.length - 1].tool}. Check
              the Shipped view before switching: cheap lines that never merge cost more.
            </Insight>
          ) : null}
        </div>
      </ConceptCard>

      <ConceptCard
        title="Model mix"
        description="Weekly gateway spend by model, top five plus other."
        readiness="live"
        readinessNote="The report already returns per-model requests and cost; weekly buckets are derivable."
      >
        <ModelMixChart rows={model.mix.rows} models={model.mix.models} />
      </ConceptCard>
    </div>
  )
}
