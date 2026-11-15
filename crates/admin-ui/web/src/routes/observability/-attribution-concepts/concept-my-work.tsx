// Concept 8, My work: a personal view instead of a leaderboard. Every comparison is against the
// person's own previous period, never against colleagues, which is what DX and DORA recommend for
// individual metrics. Platform admins pick whose view to open; everyone else sees their own.

import { useMemo, useState } from 'react'

import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { formatCount, formatTimestamp, formatUsd10000 } from '@/lib/format'

import { getMockDataset, MOCK_USERS, mockUser, shortRepository, type AgentTool } from './fixtures'
import { ContributionCalendar, PunchCard } from '@/components/attribution/heatmaps'
import {
  delta,
  headline,
  punchCard,
  ratio,
  rollupPullRequest,
  sliceWindow,
  sum,
  userCalendar,
  type ConceptWindow,
  type WindowSlice,
} from './metrics'
import {
  AI_TONE,
  AiShareBar,
  InsightCard,
  InlineMeter,
  ToolLabel,
  TrendKpi,
} from '@/components/attribution/parts'
import { formatHours, formatPercent } from '@/components/attribution/format'

function forUser(slice: WindowSlice, userId: string): WindowSlice {
  const commits = slice.commits.filter((commit) => commit.userId === userId)
  return {
    ...slice,
    commits,
    sessions: slice.sessions.filter((session) => session.userId === userId),
    pullRequests: slice.pullRequests.filter((pr) => pr.userId === userId),
    merged: slice.merged.filter((pr) => pr.userId === userId),
  }
}

// A concept page is one linear layout read top to bottom; splitting it would scatter the mockup.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function MyWorkConcept({ days }: { days: ConceptWindow }) {
  const [userId, setUserId] = useState(MOCK_USERS[0].id)
  const user = mockUser(userId)
  const model = useMemo(() => {
    const dataset = getMockDataset()
    const current = forUser(sliceWindow(dataset, days), userId)
    const previous = forUser(sliceWindow(dataset, days, 1), userId)
    const weekly = Array.from({ length: 13 }, (_, index) =>
      headline(forUser(sliceWindow(dataset, 7, 12 - index), userId)),
    )
    const toolLines = new Map<AgentTool, number>()
    for (const commit of current.commits) {
      if (commit.tool)
        toolLines.set(commit.tool, (toolLines.get(commit.tool) ?? 0) + commit.aiLines)
    }
    return {
      now: headline(current),
      before: headline(previous),
      weekly,
      calendar: userCalendar(dataset, userId, 182),
      punch: punchCard(forUser(sliceWindow(dataset, 90), userId).commits),
      tools: [...toolLines.entries()].sort((a, b) => b[1] - a[1]),
      recent: current.merged
        .map((pr) => rollupPullRequest(current, pr))
        .sort((a, b) => (b.pr.mergedAt ?? 0) - (a.pr.mergedAt ?? 0))
        .slice(0, 6),
    }
  }, [days, userId])

  const { now, before } = model
  const toolTotal = sum(model.tools, ([, lines]) => lines)

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <GeneratedAvatar kind="user" name={user.name} size={40} />
          <div>
            <p className="font-medium">{user.name}</p>
            <p className="text-muted-foreground text-sm">
              {user.team} · compared with your previous {days} days, not with anyone else
            </p>
          </div>
        </div>
        <Select value={userId} onValueChange={setUserId}>
          <SelectTrigger className="w-56" aria-label="View as">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MOCK_USERS.map((entry) => (
              <SelectItem key={entry.id} value={entry.id}>
                {entry.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <TrendKpi
          label="Your AI share"
          value={formatPercent(now.aiShare)}
          delta={delta(now.aiShare, before.aiShare)}
          spark={model.weekly.map((week) => week.aiShare)}
          tone={AI_TONE}
          readiness="live"
        />
        <TrendKpi
          label="Merged PRs"
          value={formatCount(now.mergedPrs)}
          delta={delta(now.mergedPrs, before.mergedPrs)}
          spark={model.weekly.map((week) => week.mergedPrs)}
          readiness="live"
        />
        <TrendKpi
          label="Agent spend"
          value={formatUsd10000(now.costUsd10000)}
          delta={delta(now.costUsd10000, before.costUsd10000)}
          spark={model.weekly.map((week) => week.costUsd10000)}
          hint={`${formatPercent(ratio(now.uncommittedCostUsd10000, now.costUsd10000))} never committed`}
          readiness="live"
        />
        <TrendKpi
          label="First commit to merge"
          value={formatHours(now.medianLeadTimeMs)}
          delta={delta(now.medianLeadTimeMs, before.medianLeadTimeMs)}
          spark={model.weekly.map((week) => week.medianLeadTimeMs)}
          readiness="derived"
        />
      </div>

      <InsightCard
        title="Activity"
        description="Commits per day over 26 weeks. A dot marks days where AI wrote most of your attested lines."
        readiness="derived"
        readinessNote="Bucket your live commit rows by day."
      >
        <div className="flex flex-wrap items-start gap-x-10 gap-y-6">
          <ContributionCalendar cells={model.calendar} />
          <CalendarStats cells={model.calendar} />
        </div>
      </InsightCard>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_24rem]">
        <InsightCard
          title="When you work with agents"
          description="Commits by weekday and hour (UTC) over 90 days. Brighter dots are hours where most commits were AI-written."
          readiness="derived"
        >
          <PunchCard grid={model.punch} />
        </InsightCard>
        <InsightCard
          title="Harnesses"
          description="Share of your committed AI lines."
          readiness="live"
        >
          <ul className="flex flex-col gap-3">
            {model.tools.map(([tool, lines]) => (
              <li key={tool} className="flex flex-col gap-1">
                <div className="flex items-center justify-between gap-2 text-sm">
                  <ToolLabel harness={tool} />
                  <span className="text-muted-foreground text-xs tabular-nums">
                    {formatCount(lines)} lines
                  </span>
                </div>
                <InlineMeter value={ratio(lines, toolTotal)} tone={AI_TONE} />
              </li>
            ))}
          </ul>
        </InsightCard>
      </div>

      <InsightCard
        title="Your recent merges"
        description="AI share and cost of each PR you merged in the window."
        readiness="live"
      >
        <ul className="divide-border flex flex-col divide-y">
          {model.recent.map((row) => (
            <li
              key={`${row.pr.repository}#${row.pr.number}`}
              className="grid grid-cols-[minmax(0,1fr)_12rem_6rem] items-center gap-4 py-2.5"
            >
              <div className="min-w-0">
                <p className="truncate text-sm font-medium">{row.pr.title}</p>
                <p className="text-muted-foreground text-xs">
                  <span className="font-mono">
                    {shortRepository(row.pr.repository)}#{row.pr.number}
                  </span>{' '}
                  · {formatTimestamp(new Date(row.pr.mergedAt ?? 0))}
                </p>
              </div>
              <div className="flex items-center gap-2">
                <div className="flex-1">
                  <AiShareBar
                    ai={row.aiLines}
                    human={row.humanLines}
                    unknown={row.unknownLines}
                    size="sm"
                  />
                </div>
                <span className="w-9 text-right text-xs tabular-nums">
                  {formatPercent(row.aiShare)}
                </span>
              </div>
              <span className="text-right text-sm tabular-nums">
                {row.costUsd10000 > 0 ? formatUsd10000(row.costUsd10000) : '—'}
              </span>
            </li>
          ))}
        </ul>
      </InsightCard>
    </div>
  )
}

/** Streak and day counts beside the calendar. */
function CalendarStats({ cells }: { cells: ReturnType<typeof userCalendar> }) {
  const active = cells.filter((cell) => cell.commits > 0)
  const aiDays = active.filter((cell) => (cell.aiShare ?? 0) >= 0.5)
  const byWeekday = [0, 0, 0, 0, 0, 0, 0]
  for (const cell of cells) byWeekday[cell.weekday] += Math.max(0, cell.commits)
  const busiest = byWeekday.indexOf(Math.max(...byWeekday))
  return (
    <dl className="grid grid-cols-2 gap-x-8 gap-y-4">
      <CalendarStat label="Active days" value={formatCount(active.length)} />
      <CalendarStat
        label="AI-majority days"
        value={formatPercent(ratio(aiDays.length, active.length))}
      />
      <CalendarStat
        label="Busiest weekday"
        value={
          ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'][busiest]
        }
      />
      <CalendarStat
        label="Commits per active day"
        value={(sum(active, (cell) => cell.commits) / Math.max(1, active.length)).toFixed(1)}
      />
    </dl>
  )
}

function CalendarStat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="text-lg font-medium tabular-nums">{value}</dd>
      {hint ? <dd className="text-muted-foreground text-xs">{hint}</dd> : null}
    </div>
  )
}
