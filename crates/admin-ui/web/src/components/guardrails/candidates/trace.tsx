import { useState } from 'react'

import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import {
  Timeline,
  TimelineContent,
  TimelineDate,
  TimelineHeader,
  TimelineIndicator,
  TimelineItem,
  TimelineSeparator,
  TimelineTitle,
} from '@/components/reui/timeline'
import { Badge } from '@/components/ui/badge'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import type { GuardrailDecisionView } from '@/types/api'

import {
  actionLabel,
  actionVariant,
  formatClock,
  formatLatency,
  formatTimestamp,
  phaseLabel,
  policyOverrides,
  policyScopes,
  redactionLabel,
} from '../decision-meta'
import { DecisionSheet } from '../decision-sheet'
import { DecisionPager, DecisionToolbar, type CandidateProps } from '../decision-toolbar'

const SEVERITY = ['allow', 'audit', 'transformed', 'deny']

const INDICATOR: Record<string, string> = {
  allow: 'border-success bg-success/24',
  audit: 'border-warning bg-warning/24',
  transformed: 'border-chart-2 bg-chart-2/24',
  deny: 'border-destructive bg-destructive/10',
}

type RequestTrace = {
  key: string
  requestId: string | null
  decisions: GuardrailDecisionView[]
  outcome: string
}

/** Request traces: decisions grouped per request and laid out in the order guardrails ran. */
export function TraceCandidate(props: CandidateProps) {
  const { decisions, policies, onSearch } = props
  const [selected, setSelected] = useState<GuardrailDecisionView | null>(null)
  const traces = groupByRequest(decisions.items)

  return (
    <div className="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,1fr)_18rem]">
      <Frame spacing="default" stacked className="min-w-0">
        <FrameHeader className="flex flex-col gap-3">
          <div className="flex flex-col gap-0.5">
            <FrameTitle>Request traces</FrameTitle>
            <FrameDescription>Each request's guardrail checks, oldest first.</FrameDescription>
          </div>
          <DecisionToolbar {...props} />
        </FrameHeader>
        {traces.length === 0 ? (
          <FramePanel>
            <Empty>
              <EmptyHeader>
                <EmptyTitle>No guardrail decisions</EmptyTitle>
                <EmptyDescription>No events match the current filters.</EmptyDescription>
              </EmptyHeader>
            </Empty>
          </FramePanel>
        ) : (
          traces.map((trace) => (
            <FramePanel key={trace.key} className="flex min-w-0 flex-col gap-4">
              <div className="flex flex-wrap items-center gap-2 text-sm">
                {trace.requestId ? (
                  <button
                    type="button"
                    className="max-w-64 truncate font-mono text-xs hover:underline"
                    title="Show only this request"
                    onClick={() => onSearch({ request_id: trace.requestId ?? undefined })}
                  >
                    {trace.requestId}
                  </button>
                ) : (
                  <span className="text-muted-foreground text-xs">No request ID</span>
                )}
                <Badge variant={actionVariant(trace.outcome)}>{actionLabel(trace.outcome)}</Badge>
                <span className="text-muted-foreground ml-auto text-xs tabular-nums">
                  {formatTimestamp(trace.decisions[0].occurred_at)}
                </span>
              </div>
              <div className="overflow-x-auto pb-1">
                <Timeline value={0} orientation="horizontal" className="min-w-max">
                  {trace.decisions.map((decision, index) => (
                    <TimelineItem key={decision.decision_id} step={index + 1} className="w-44">
                      <TimelineHeader>
                        <TimelineSeparator />
                        <TimelineDate dateTime={decision.occurred_at} className="tabular-nums">
                          {formatClock(decision.occurred_at)}
                        </TimelineDate>
                        <TimelineTitle>
                          <button
                            type="button"
                            className="hover:underline"
                            onClick={() => setSelected(decision)}
                          >
                            {phaseLabel(decision.phase)}
                          </button>
                        </TimelineTitle>
                        <TimelineIndicator className={INDICATOR[decision.action] ?? ''} />
                      </TimelineHeader>
                      <TimelineContent className="flex flex-col items-start gap-1">
                        <Badge variant={actionVariant(decision.action)}>
                          {actionLabel(decision.action)}
                        </Badge>
                        <span className="w-full truncate text-xs" title={decision.reason_code}>
                          {decision.reason_code}
                        </span>
                        <span className="text-xs tabular-nums">
                          {formatLatency(decision.latency_micros)}
                        </span>
                      </TimelineContent>
                    </TimelineItem>
                  ))}
                </Timeline>
              </div>
            </FramePanel>
          ))
        )}
        <FrameFooter>
          <DecisionPager {...props} />
        </FrameFooter>
      </Frame>

      <PolicyCoverage policies={policies} />
      <DecisionSheet decision={selected} onOpenChange={(open) => !open && setSelected(null)} />
    </div>
  )
}

function PolicyCoverage({ policies }: { policies: CandidateProps['policies'] }) {
  return (
    <Frame spacing="default" stacked className="self-start">
      <FrameHeader>
        <FrameTitle>Policy coverage</FrameTitle>
        <FrameDescription>What each scope changes from the default.</FrameDescription>
      </FrameHeader>
      {policyScopes(policies).map((scope) => {
        const overrides =
          scope.kind === 'global' ? [] : policyOverrides(scope.policy, policies.default)
        return (
          <FramePanel key={scope.key} className="flex flex-col gap-2">
            <div className="flex items-center gap-2 text-sm">
              <span className="truncate font-medium">{scope.name}</span>
              <Badge variant={scope.policy.enabled ? 'success' : 'outline'}>
                {scope.policy.enabled ? scope.policy.mode : 'Disabled'}
              </Badge>
            </div>
            {scope.kind === 'global' ? (
              <p className="text-muted-foreground text-xs">
                Packs {scope.policy.packs.join(', ') || 'none'}; redaction{' '}
                {redactionLabel(scope.policy)}.
              </p>
            ) : overrides.length ? (
              <div className="flex flex-wrap gap-1">
                {overrides.map((item) => (
                  <Badge key={item} variant="outline">
                    {item}
                  </Badge>
                ))}
              </div>
            ) : (
              <p className="text-muted-foreground text-xs">Inherits the global default.</p>
            )}
          </FramePanel>
        )
      })}
    </Frame>
  )
}

/** Groups keep the page's newest-first order; steps inside a group run oldest first. */
function groupByRequest(items: GuardrailDecisionView[]): RequestTrace[] {
  const groups = new Map<string, GuardrailDecisionView[]>()
  for (const item of items) {
    const key = item.request_id ?? `decision:${item.decision_id}`
    groups.set(key, [...(groups.get(key) ?? []), item])
  }
  return [...groups.entries()].map(([key, group]) => {
    const ordered = [...group].sort((a, b) => a.occurred_at.localeCompare(b.occurred_at))
    const outcome = ordered.reduce(
      (worst, item) =>
        SEVERITY.indexOf(item.action) > SEVERITY.indexOf(worst) ? item.action : worst,
      'allow',
    )
    return { key, requestId: ordered[0].request_id ?? null, decisions: ordered, outcome }
  })
}
