import { useTransition } from 'react'
import { createFileRoute, Link, useRouter } from '@tanstack/react-router'

import { PipelineCandidate } from '@/components/guardrails/candidates/pipeline'
import { PulseCandidate } from '@/components/guardrails/candidates/pulse'
import { ScopesCandidate } from '@/components/guardrails/candidates/scopes'
import { TraceCandidate } from '@/components/guardrails/candidates/trace'
import type { CandidateProps } from '@/components/guardrails/decision-toolbar'
import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { getGuardrailDecisionPage, getGuardrailPolicyView } from '@/server/admin-data.functions'
import type { GuardrailDecisionFiltersInput } from '@/types/api'

/**
 * Design candidates for the guardrails page. Each view reads the same loader data so they can be
 * compared side by side; the chosen one replaces `/observability/guardrails` and this route goes.
 */
const CANDIDATES = {
  pulse: { label: 'Pulse', component: PulseCandidate },
  trace: { label: 'Trace', component: TraceCandidate },
  pipeline: { label: 'Pipeline', component: PipelineCandidate },
  scopes: { label: 'Scopes', component: ScopesCandidate },
} as const

type CandidateId = keyof typeof CANDIDATES
type LabSearch = GuardrailDecisionFiltersInput & { view?: CandidateId }

/** Aggregate views need more than one screen of rows to say anything useful. */
const LAB_PAGE_SIZE = 100

export const Route = createFileRoute('/observability/guardrails-lab')({
  validateSearch: (search: Record<string, unknown>) => normalizeLabSearch(search),
  loaderDeps: ({ search }) => search,
  loader: async ({ deps }) => {
    const { view: _view, ...filters } = deps
    const [policies, decisions] = await Promise.all([
      getGuardrailPolicyView(),
      getGuardrailDecisionPage({ data: { page_size: LAB_PAGE_SIZE, ...filters } }),
    ])
    return { policies: policies.data, decisions: decisions.data }
  },
  component: GuardrailsLabPage,
})

export function GuardrailsLabPage() {
  const { policies, decisions } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [pending, startTransition] = useTransition()
  const view: CandidateId = search.view ?? 'pulse'
  const { view: _view, ...filters } = search
  const Candidate = CANDIDATES[view].component

  const navigate = (next: LabSearch) =>
    startTransition(async () => {
      await router.navigate({ to: '/observability/guardrails-lab', search: next })
    })
  const props: CandidateProps = {
    policies,
    decisions,
    search: filters,
    pending,
    onSearch: (next) => navigate({ ...next, view }),
    onPage: (page) => navigate({ ...filters, page, view }),
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Guardrails"
        description="Design candidates for the guardrails page, all reading the same live data."
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              aria-label="Design candidate"
              value={view}
              onValueChange={(value) =>
                value && navigate({ ...filters, view: value as CandidateId })
              }
            >
              {(Object.keys(CANDIDATES) as CandidateId[]).map((id) => (
                <ToggleGroupItem key={id} value={id}>
                  {CANDIDATES[id].label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <Button variant="outline" size="sm" asChild>
              <Link to="/observability/guardrails">Current page</Link>
            </Button>
          </div>
        }
      />
      <Candidate {...props} />
    </div>
  )
}

function normalizeLabSearch(search: Record<string, unknown>): LabSearch {
  const result: LabSearch = {}
  for (const key of [
    'request_id',
    'phase',
    'action',
    'evaluator',
    'occurred_at_start',
    'occurred_at_end',
  ] as const) {
    const value = search[key]
    if (typeof value === 'string' && value.trim()) result[key] = value.trim()
  }
  for (const key of ['page', 'page_size'] as const) {
    const value = search[key]
    if (typeof value === 'number' && Number.isInteger(value) && value > 0) result[key] = value
  }
  if (typeof search.view === 'string' && search.view in CANDIDATES) {
    result.view = search.view as CandidateId
  }
  return result
}
