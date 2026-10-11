import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import type { GuardrailDecisionView } from '@/types/api'

import {
  actionLabel,
  actionVariant,
  evaluatorLabel,
  formatLatency,
  formatTimestamp,
  phaseLabel,
  ruleLabel,
} from './decision-meta'

/** Read-only detail for one decision. Content hashes stand in for the raw text, which is never stored. */
export function DecisionSheet({
  decision,
  onOpenChange,
}: {
  decision: GuardrailDecisionView | null
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Sheet open={decision !== null} onOpenChange={onOpenChange}>
      <SheetContent className="sm:max-w-md">
        {decision ? (
          <>
            <SheetHeader>
              <SheetTitle className="flex items-center gap-2">
                {phaseLabel(decision.phase)}
                <Badge variant={actionVariant(decision.action)}>
                  {actionLabel(decision.action)}
                </Badge>
              </SheetTitle>
              <SheetDescription>{formatTimestamp(decision.occurred_at)}</SheetDescription>
            </SheetHeader>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-3 px-4 text-sm">
              <Fact label="Reason">{decision.reason_code}</Fact>
              <Fact label="Rule">{ruleLabel(decision)}</Fact>
              <Fact label="Evaluator">{evaluatorLabel(decision)}</Fact>
              <Fact label="Scope">{decision.effective_scope}</Fact>
              <Fact label="Latency">
                <span className="tabular-nums">{formatLatency(decision.latency_micros)}</span>
              </Fact>
              {decision.failure_disposition ? (
                <Fact label="On failure">{decision.failure_disposition}</Fact>
              ) : null}
              <Fact label="Transformed">{decision.transformed ? 'Yes' : 'No'}</Fact>
            </dl>
            <Separator />
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-3 px-4 text-sm">
              <Fact label="Decision">
                <Mono>{decision.decision_id}</Mono>
              </Fact>
              <Fact label="Request">
                <Mono>{decision.request_id ?? '—'}</Mono>
              </Fact>
              {decision.mcp_tool_invocation_id ? (
                <Fact label="MCP call">
                  <Mono>{decision.mcp_tool_invocation_id}</Mono>
                </Fact>
              ) : null}
              <Fact label="Content">
                <Mono>{decision.content_hash}</Mono>
              </Fact>
            </dl>
            <p className="text-muted-foreground px-4 text-xs">
              Raw prompts, commands, arguments, and results are not stored.
            </p>
          </>
        ) : null}
      </SheetContent>
    </Sheet>
  )
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </>
  )
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span className="font-mono text-xs break-all">{children}</span>
}
