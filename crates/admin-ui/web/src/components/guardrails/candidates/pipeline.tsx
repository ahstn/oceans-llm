import { useState } from 'react'

import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { GuardrailDecisionView } from '@/types/api'

import { ActionLegend, ActionMeter } from '../action-meter'
import {
  PHASES,
  actionLabel,
  actionVariant,
  countBy,
  formatClock,
  policyScopes,
} from '../decision-meta'
import { DecisionSheet } from '../decision-sheet'
import { DecisionPager, DecisionToolbar, type CandidateProps } from '../decision-toolbar'

const COLUMN_LIMIT = 6

/** Pipeline board: one column per guardrail phase, in the order a request passes through them. */
export function PipelineCandidate(props: CandidateProps) {
  const { decisions, policies, search, onSearch } = props
  const [selected, setSelected] = useState<GuardrailDecisionView | null>(null)
  const [scope, setScope] = useState('all')
  const scopes = policyScopes(policies)
  const items = decisions.items.filter((item) => scope === 'all' || item.effective_scope === scope)
  const phases = search.phase ? PHASES.filter(({ id }) => id === search.phase) : PHASES

  return (
    <Frame spacing="default" className="min-w-0">
      <FrameHeader className="flex flex-col gap-3">
        <div className="flex flex-wrap items-end justify-between gap-2">
          <div className="flex flex-col gap-0.5">
            <FrameTitle>Guardrail pipeline</FrameTitle>
            <FrameDescription>
              Decisions on this page, by the phase that made them.
            </FrameDescription>
          </div>
          <ActionLegend />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <DecisionToolbar {...props} showPhase={false} />
          {scopes.length > 1 ? (
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              aria-label="Scope"
              value={scope}
              onValueChange={(value) => setScope(value || 'all')}
            >
              <ToggleGroupItem value="all">All scopes</ToggleGroupItem>
              {scopes.map((item) => (
                <ToggleGroupItem key={item.key} value={item.key}>
                  {item.kind === 'global' ? 'Global' : item.name}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          ) : null}
        </div>
      </FrameHeader>
      {items.length === 0 ? (
        <FramePanel>
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No guardrail decisions</EmptyTitle>
              <EmptyDescription>No events match the current filters.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        </FramePanel>
      ) : (
        <div className="grid min-w-0 auto-cols-[minmax(15rem,1fr)] grid-flow-col gap-(--frame-gap) overflow-x-auto">
          {phases.map(({ id, label }) => {
            const column = items.filter((item) => item.phase === id)
            const expanded = search.phase === id
            const shown = expanded ? column : column.slice(0, COLUMN_LIMIT)
            return (
              <FramePanel key={id} className="flex min-w-0 flex-col gap-3">
                <div className="flex items-center justify-between gap-2 text-sm">
                  <span className="truncate font-medium">{label}</span>
                  <Badge variant="outline">{column.length}</Badge>
                </div>
                <ActionMeter counts={countBy(column, (item) => item.action)} />
                {column.length === 0 ? (
                  <p className="text-muted-foreground text-xs">No decisions in this phase.</p>
                ) : (
                  <ul className="-mx-2 flex flex-col">
                    {shown.map((decision) => (
                      <li key={decision.decision_id}>
                        <button
                          type="button"
                          onClick={() => setSelected(decision)}
                          className="hover:bg-muted dark:hover:bg-muted/50 flex w-full min-w-0 flex-col gap-1 rounded-md px-2 py-1.5 text-left transition-colors"
                        >
                          <span className="flex items-center gap-2">
                            <Badge variant={actionVariant(decision.action)}>
                              {actionLabel(decision.action)}
                            </Badge>
                            <span className="text-muted-foreground ml-auto text-xs tabular-nums">
                              {formatClock(decision.occurred_at)}
                            </span>
                          </span>
                          <span className="truncate text-sm">{decision.reason_code}</span>
                          {decision.rule_id ? (
                            <span className="text-muted-foreground truncate font-mono text-xs">
                              {decision.rule_id}
                            </span>
                          ) : null}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                {column.length > shown.length ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onSearch({ ...search, phase: id, page: undefined })}
                  >
                    {column.length - shown.length} more
                  </Button>
                ) : null}
              </FramePanel>
            )
          })}
        </div>
      )}
      <FrameFooter>
        <DecisionPager {...props} />
      </FrameFooter>
      <DecisionSheet decision={selected} onOpenChange={(open) => !open && setSelected(null)} />
    </Frame>
  )
}
