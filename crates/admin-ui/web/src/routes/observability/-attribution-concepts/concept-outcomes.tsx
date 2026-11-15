// Concept 2, Session outcomes: where agent work ends up. A Sankey follows every session to its
// furthest stage (no commit, branch, PR, merged), sized by sessions or by spend, and the list beside
// it names the costliest work that never shipped, which is where harness fixes pay off first.

import { useMemo, useState } from 'react'

import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { formatCount, formatTimestamp, formatUsd10000 } from '@/lib/format'

import { FlowChart, type FlowLink, type FlowNode } from './flow-chart'
import { getMockDataset, mockUser, shortRepository } from './fixtures'
import {
  ratio,
  SESSION_OUTCOME_LABELS,
  sessionOutcomes,
  sliceWindow,
  sum,
  type ConceptWindow,
  type SessionOutcome,
} from './metrics'
import {
  ConceptCard,
  formatPercent,
  Insight,
  ReadinessBadge,
  SHIPPED_TONE,
  ToolLabel,
  UNSHIPPED_TONE,
} from './parts'

type Measure = 'sessions' | 'cost'

const STAGE_TONES: Record<SessionOutcome | 'all' | 'committed' | 'pr', string> = {
  all: 'var(--chart-3)',
  committed: 'var(--chart-1)',
  pr: 'var(--chart-5)',
  merged: SHIPPED_TONE,
  direct: SHIPPED_TONE,
  in_review: 'var(--chart-2)',
  branch: 'var(--chart-other)',
  closed: UNSHIPPED_TONE,
  no_commit: UNSHIPPED_TONE,
}

export default function OutcomesConcept({ days }: { days: ConceptWindow }) {
  const [measure, setMeasure] = useState<Measure>('cost')
  const model = useMemo(() => {
    const dataset = getMockDataset()
    const slice = sliceWindow(dataset, days)
    return { slice, ...sessionOutcomes(dataset, slice) }
  }, [days])

  const flow = useMemo(() => buildFlow(model.tally, measure), [model.tally, measure])
  const unshipped = model.rows
    .filter(({ outcome }) => outcome === 'no_commit' || outcome === 'closed')
    .sort((a, b) => b.session.costUsd10000 - a.session.costUsd10000)
    .slice(0, 7)
  const total = sum([...model.tally.values()], (entry) => entry.costUsd10000)
  const lost =
    (model.tally.get('no_commit')?.costUsd10000 ?? 0) +
    (model.tally.get('closed')?.costUsd10000 ?? 0)

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <ConceptCard
          title="Session outcomes"
          description="Every agent session in the window, followed to the furthest stage its commits reached."
          readiness="derived"
          readinessNote="Hook-reported sessions, their commits and merged PRs are all stored; the flow needs one aggregation."
          action={
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={measure}
              onValueChange={(next) => next && setMeasure(next as Measure)}
              aria-label="Size flows by"
            >
              <ToggleGroupItem value="cost">Spend</ToggleGroupItem>
              <ToggleGroupItem value="sessions">Sessions</ToggleGroupItem>
            </ToggleGroup>
          }
        >
          <div className="flex flex-col gap-4">
            <FlowChart
              nodes={flow.nodes}
              links={flow.links}
              columns={flow.columns}
              ariaLabel="Agent sessions by outcome"
            />
            <Insight>
              {formatPercent(ratio(lost, total))} of agent spend ({formatUsd10000(lost)}) went to
              sessions that never committed or whose PR closed unmerged. Git AI reports about 38%
              for typical teams.
            </Insight>
          </div>
        </ConceptCard>

        <ConceptCard
          title="Costliest unshipped sessions"
          description="Where to look for missing context, flaky tests or the wrong model."
          readiness="derived"
          contentClassName="px-0"
        >
          <ul className="divide-border flex flex-col divide-y">
            {unshipped.map(({ session, outcome }) => (
              <li key={session.id} className="flex flex-col gap-1 px-4 py-2.5">
                <div className="flex items-center justify-between gap-3">
                  <span className="truncate text-sm font-medium">
                    {mockUser(session.userId).name}
                  </span>
                  <span className="text-sm font-medium tabular-nums">
                    {formatUsd10000(session.costUsd10000)}
                  </span>
                </div>
                <div className="text-muted-foreground flex items-center justify-between gap-3 text-xs">
                  <span className="flex min-w-0 items-center gap-2">
                    <ToolLabel tool={session.tool} hideName />
                    <span className="truncate font-mono">
                      {shortRepository(session.repository)}
                    </span>
                  </span>
                  <span className="whitespace-nowrap">{SESSION_OUTCOME_LABELS[outcome]}</span>
                </div>
                <div className="text-muted-foreground flex items-center justify-between gap-3 text-xs">
                  <span className="whitespace-nowrap">
                    {formatTimestamp(new Date(session.startedAt))}
                  </span>
                  <span className="tabular-nums">
                    {formatCount(session.requests)} requests · {formatCount(session.steeringTurns)}{' '}
                    steers
                  </span>
                </div>
              </li>
            ))}
          </ul>
        </ConceptCard>
      </div>

      <ConceptCard
        title="AI code journey"
        description="Lines through each stage from generation to 30 days after merge. Each drop names what removed the lines."
        readiness="new"
        readinessNote="Generated lines need checkpoint telemetry from git-ai, and durability needs line survival. Committed and merged are derivable today."
      >
        <LineJourney slice={model.slice} />
      </ConceptCard>
    </div>
  )
}

function buildFlow(
  tally: Map<SessionOutcome, { sessions: number; costUsd10000: number }>,
  measure: Measure,
) {
  const get = (outcome: SessionOutcome) => {
    const entry = tally.get(outcome) ?? { sessions: 0, costUsd10000: 0 }
    return measure === 'cost' ? entry.costUsd10000 : entry.sessions
  }
  const sessionsOf = (outcome: SessionOutcome) => tally.get(outcome)?.sessions ?? 0
  const costOf = (outcome: SessionOutcome) => tally.get(outcome)?.costUsd10000 ?? 0
  const all: SessionOutcome[] = ['merged', 'in_review', 'closed', 'direct', 'branch', 'no_commit']
  const total = sum(all, get)
  const totalSessions = sum(all, sessionsOf)
  const totalCost = sum(all, costOf)
  const pr: SessionOutcome[] = ['merged', 'in_review', 'closed']
  const committed: SessionOutcome[] = [...pr, 'direct', 'branch']
  const detail = (outcomes: SessionOutcome[]) => {
    const sessions = sum(outcomes, sessionsOf)
    const cost = sum(outcomes, costOf)
    return `${formatCount(sessions)} · ${formatPercent(ratio(sessions, totalSessions))} · ${formatUsd10000(cost)}`
  }
  const node = (
    id: string,
    label: string,
    column: number,
    outcomes: SessionOutcome[],
    tone: string,
    leak = false,
  ): FlowNode => ({
    id,
    label,
    column,
    value: sum(outcomes, get),
    tone,
    detail: detail(outcomes),
    leak,
  })
  const nodes: FlowNode[] = [
    node('all', 'All sessions', 0, all, STAGE_TONES.all),
    node('committed', 'Committed', 1, committed, STAGE_TONES.committed),
    node('no_commit', 'No commit', 1, ['no_commit'], STAGE_TONES.no_commit, true),
    node('pr', 'Pull request', 2, pr, STAGE_TONES.pr),
    node('direct', 'Pushed to main', 2, ['direct'], STAGE_TONES.direct),
    node('branch', 'Branch, no PR yet', 2, ['branch'], STAGE_TONES.branch, true),
    node('merged', 'Merged', 3, ['merged'], STAGE_TONES.merged),
    node('in_review', 'In review', 3, ['in_review'], STAGE_TONES.in_review),
    node('closed', 'Closed unmerged', 3, ['closed'], STAGE_TONES.closed, true),
  ].filter((entry) => entry.value > 0)
  const links: FlowLink[] = [
    { source: 'all', target: 'committed', value: sum(committed, get) },
    { source: 'all', target: 'no_commit', value: get('no_commit') },
    { source: 'committed', target: 'pr', value: sum(pr, get) },
    { source: 'committed', target: 'direct', value: get('direct') },
    { source: 'committed', target: 'branch', value: get('branch') },
    { source: 'pr', target: 'merged', value: get('merged') },
    { source: 'pr', target: 'in_review', value: get('in_review') },
    { source: 'pr', target: 'closed', value: get('closed') },
  ].filter((link) => link.value > 0)
  const share = (outcomes: SessionOutcome[]) => formatPercent(ratio(sum(outcomes, get), total))
  return {
    nodes,
    links,
    columns: [
      {
        title: 'Started',
        value: measure === 'cost' ? formatUsd10000(totalCost) : formatCount(totalSessions),
      },
      { title: 'Committed', value: share(committed) },
      { title: 'Opened a PR', value: share(pr) },
      { title: 'Shipped', value: share(['merged', 'direct']) },
    ],
  }
}

/** Generated → committed → merged → durable, as shrinking bars with the drop between each. */
function LineJourney({ slice }: { slice: ReturnType<typeof sliceWindow> }) {
  const generated = sum(slice.sessions, (session) => session.generatedLines)
  const committed = sum(slice.commits, (commit) => commit.aiLines)
  const mergedShas = new Set(slice.merged.flatMap((pr) => pr.commitShas))
  const merged = sum(
    slice.commits.filter((commit) => mergedShas.has(commit.sha) || commit.isDefaultBranch),
    (commit) => commit.aiLines,
  )
  const durable = sum(slice.merged, (pr) => pr.aiLinesDurable30)
  const stages = [
    { label: 'Generated', value: generated, drop: null, readiness: 'new' as const },
    {
      label: 'Committed',
      value: committed,
      drop: 'discarded by the agent or human before commit',
      readiness: 'derived' as const,
    },
    {
      label: 'Merged',
      value: merged,
      drop: 'still in review, on a branch, or closed',
      readiness: 'derived' as const,
    },
    {
      label: 'Unchanged after 30 days',
      value: durable,
      drop: 'rewritten, reverted or refactored after merge',
      readiness: 'new' as const,
    },
  ]
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:grid-cols-4">
        {stages.map((stage, index) => {
          const previous = index > 0 ? stages[index - 1].value : null
          return (
            <div key={stage.label} className="flex min-w-0 flex-col gap-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-muted-foreground text-xs">{stage.label}</span>
                <ReadinessBadge level={stage.readiness} />
              </div>
              <span className="text-2xl font-medium tabular-nums">{formatCount(stage.value)}</span>
              <div className="bg-muted h-2 overflow-hidden rounded-full">
                <div
                  className="h-full rounded-full bg-(--chart-1)"
                  style={{ width: `${(stage.value / Math.max(1, generated)) * 100}%` }}
                />
              </div>
              {previous != null && stage.drop ? (
                <span className="text-muted-foreground text-xs">
                  <span className="text-foreground tabular-nums">
                    −{formatCount(previous - stage.value)}
                  </span>{' '}
                  {stage.drop}
                </span>
              ) : (
                <span className="text-muted-foreground text-xs">
                  Lines agents wrote in their sessions
                </span>
              )}
            </div>
          )
        })}
      </div>
      <div className="text-muted-foreground flex flex-wrap gap-x-6 gap-y-1 border-t pt-3 text-xs">
        <span>
          <span className="text-foreground font-medium tabular-nums">
            {Math.round(generated / Math.max(1, durable))} : 1
          </span>{' '}
          generated to durable
        </span>
        <span>
          <span className="text-foreground font-medium tabular-nums">
            {formatPercent(ratio(generated - committed, generated))}
          </span>{' '}
          discarded before commit
        </span>
        <span>
          <span className="text-foreground font-medium tabular-nums">
            {formatPercent(ratio(merged - durable, merged))}
          </span>{' '}
          of merged AI lines changed within 30 days
        </span>
      </div>
    </div>
  )
}
