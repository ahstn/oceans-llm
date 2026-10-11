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
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import type { GuardrailDecisionView, GuardrailPoliciesView } from '@/types/api'

import { ActionMeter } from '../action-meter'
import {
  actionLabel,
  actionVariant,
  countBy,
  formatClock,
  phaseLabel,
  policyOverrides,
  policyScopes,
  type PolicyScope,
} from '../decision-meta'
import { DecisionSheet } from '../decision-sheet'
import { DecisionPager, DecisionToolbar, type CandidateProps } from '../decision-toolbar'

const KIND_LABEL: Record<PolicyScope['kind'], string> = {
  global: 'Default',
  model: 'Model route',
  mcp: 'MCP server',
}

/** Policy-first: pick a scope on the rail to see its effective policy and the decisions it made. */
export function ScopesCandidate(props: CandidateProps) {
  const { decisions, policies } = props
  const scopes = policyScopes(policies)
  const [scopeKey, setScopeKey] = useState(scopes[0].key)
  const [selected, setSelected] = useState<GuardrailDecisionView | null>(null)
  const scope = scopes.find((item) => item.key === scopeKey) ?? scopes[0]
  const scoped = decisions.items.filter((item) => item.effective_scope === scope.key)

  return (
    <div className="grid min-w-0 gap-6 md:grid-cols-[15rem_minmax(0,1fr)]">
      <nav aria-label="Policy scopes" className="flex min-w-0 gap-1 overflow-x-auto md:flex-col">
        {scopes.map((item) => {
          const count = decisions.items.filter((d) => d.effective_scope === item.key)
          return (
            <button
              key={item.key}
              type="button"
              aria-current={item.key === scope.key ? 'true' : undefined}
              onClick={() => setScopeKey(item.key)}
              className="hover:bg-muted aria-[current=true]:bg-muted dark:hover:bg-muted/50 dark:aria-[current=true]:bg-muted/50 flex min-w-44 flex-col gap-1.5 rounded-md px-3 py-2 text-left transition-colors md:min-w-0"
            >
              <span className="flex items-center gap-2 text-sm">
                <span className="truncate font-medium">{item.name}</span>
                <span className="text-muted-foreground ml-auto text-xs tabular-nums">
                  {count.length}
                </span>
              </span>
              <span className="text-muted-foreground text-xs">{KIND_LABEL[item.kind]}</span>
              <ActionMeter counts={countBy(count, (d) => d.action)} className="h-1" />
            </button>
          )
        })}
      </nav>

      <div className="flex min-w-0 flex-col gap-6">
        <Frame spacing="default" className="min-w-0">
          <FrameHeader className="flex-row flex-wrap items-center gap-2">
            <FrameTitle>{scope.name}</FrameTitle>
            <Badge variant={scope.policy.enabled ? 'success' : 'outline'}>
              {scope.policy.enabled ? 'Enabled' : 'Disabled'}
            </Badge>
            <Badge variant="outline">{scope.policy.mode}</Badge>
            <FrameDescription className="basis-full">
              {scope.kind === 'global'
                ? 'Applies wherever no route or server overrides it.'
                : overrideSummary(scope, policies)}
            </FrameDescription>
          </FrameHeader>
          <FramePanel>
            <PolicyFacts scope={scope} policies={policies} />
          </FramePanel>
        </Frame>

        <Frame spacing="default" className="min-w-0">
          <FrameHeader className="flex flex-col gap-3">
            <div className="flex flex-col gap-0.5">
              <FrameTitle>Decisions in this scope</FrameTitle>
              <FrameDescription>From the current page of results.</FrameDescription>
            </div>
            <DecisionToolbar {...props} />
          </FrameHeader>
          <FramePanel className="flex flex-col">
            {scoped.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyTitle>No decisions for {scope.name}</EmptyTitle>
                  <EmptyDescription>
                    Nothing on this page was decided under this scope.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <ul className="-mx-2 flex flex-col">
                {scoped.map((decision) => (
                  <li key={decision.decision_id}>
                    <button
                      type="button"
                      onClick={() => setSelected(decision)}
                      className="hover:bg-muted dark:hover:bg-muted/50 grid w-full grid-cols-[4.5rem_6.5rem_minmax(0,1fr)] items-center gap-3 rounded-md px-2 py-1.5 text-left text-sm transition-colors sm:grid-cols-[4.5rem_6.5rem_10rem_minmax(0,1fr)]"
                    >
                      <span className="text-muted-foreground text-xs tabular-nums">
                        {formatClock(decision.occurred_at)}
                      </span>
                      <Badge variant={actionVariant(decision.action)}>
                        {actionLabel(decision.action)}
                      </Badge>
                      <span className="hidden truncate sm:block">{phaseLabel(decision.phase)}</span>
                      <span className="truncate">{decision.reason_code}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </FramePanel>
          <FrameFooter>
            <DecisionPager {...props} />
          </FrameFooter>
        </Frame>
      </div>
      <DecisionSheet decision={selected} onOpenChange={(open) => !open && setSelected(null)} />
    </div>
  )
}

function overrideSummary(scope: PolicyScope, policies: GuardrailPoliciesView) {
  const changed = policyOverrides(scope.policy, policies.default)
  return changed.length
    ? `Overrides ${changed.join(', ')}.`
    : 'Inherits every setting from the default.'
}

function PolicyFacts({ scope, policies }: { scope: PolicyScope; policies: GuardrailPoliciesView }) {
  const { policy } = scope
  const changed = new Set(scope.kind === 'global' ? [] : policyOverrides(policy, policies.default))
  const checks = policies.managed_checks.filter((check) =>
    policy.managed_checks.includes(check.name),
  )
  return (
    <dl className="grid gap-4 text-sm sm:grid-cols-2">
      <Fact label="Packs" changed={changed.has('packs')}>
        {policy.packs.length ? (
          <span className="flex flex-wrap gap-1">
            {policy.packs.map((pack) => (
              <Badge key={pack} variant="outline">
                {pack}
                {versionOf(pack, policies) ? ` v${versionOf(pack, policies)}` : ''}
              </Badge>
            ))}
          </span>
        ) : (
          'None'
        )}
      </Fact>
      <Fact label="Secret redaction" changed={changed.has('secret redaction')}>
        {policy.secret_redaction.enabled ? (
          <span className="flex flex-wrap gap-1">
            {policy.secret_redaction.tiers.map((tier) => (
              <Badge key={tier} variant="outline">
                {tier}
              </Badge>
            ))}
            {policy.secret_redaction.disabled_rules.length ? (
              <span className="text-muted-foreground text-xs">
                {policy.secret_redaction.disabled_rules.length} rules disabled
              </span>
            ) : null}
          </span>
        ) : (
          'Off'
        )}
      </Fact>
      <Fact label="Managed checks" changed={changed.has('managed checks')}>
        {checks.length ? (
          <span className="flex flex-col gap-1">
            {checks.map((check) => (
              <span key={check.name} className="flex min-w-0 flex-col">
                <span className="truncate">{check.name}</span>
                <span className="text-muted-foreground truncate text-xs">
                  {check.kind}, {check.timeout_ms} ms timeout, fails {check.failure_disposition}
                </span>
              </span>
            ))}
          </span>
        ) : (
          policy.managed_checks.join(', ') || 'None'
        )}
      </Fact>
      <Fact label="Stream buffer" changed={changed.has('stream buffer')}>
        <span className="tabular-nums">{formatBytes(policy.stream_buffer_bytes)}</span>
      </Fact>
    </dl>
  )
}

function Fact({
  label,
  changed,
  children,
}: {
  label: string
  changed: boolean
  children: React.ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <dt className="text-muted-foreground flex items-center gap-2 text-xs">
        {label}
        {changed ? <Badge variant="warning">Override</Badge> : null}
      </dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  )
}

function versionOf(pack: string, policies: GuardrailPoliciesView) {
  return policies.built_in_packs.find((item) => item.id === pack)?.version
}

function formatBytes(bytes: number) {
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(bytes % 1_048_576 ? 1 : 0)} MiB`
  if (bytes >= 1_024) return `${Math.round(bytes / 1_024)} KiB`
  return `${bytes} B`
}
