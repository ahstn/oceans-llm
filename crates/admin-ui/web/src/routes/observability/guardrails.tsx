import { useTransition } from 'react'
import { createFileRoute, Link, useRouter } from '@tanstack/react-router'
import { Settings02Icon } from '@hugeicons/core-free-icons'

import { GuardrailsDashboard } from '@/components/guardrails/guardrails-dashboard'
import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { Button } from '@/components/ui/button'
import { getGuardrailDecisionPage, getGuardrailPolicyView } from '@/server/admin-data.functions'
import type { GuardrailDecisionFiltersInput } from '@/types/api'

export const Route = createFileRoute('/observability/guardrails')({
  validateSearch: (search: Record<string, unknown>) => normalizeSearch(search),
  loaderDeps: ({ search }) => search,
  loader: async ({ deps }) => {
    const [policies, decisions] = await Promise.all([
      getGuardrailPolicyView(),
      getGuardrailDecisionPage({ data: deps }),
    ])
    return { policies: policies.data, decisions: decisions.data }
  },
  component: GuardrailsPage,
})

export function GuardrailsPage() {
  const { policies, decisions } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [pending, startTransition] = useTransition()

  const navigate = (next: GuardrailDecisionFiltersInput) =>
    startTransition(async () => {
      await router.navigate({ to: '/observability/guardrails', search: next })
    })

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Guardrails"
        description="Review effective policies and privacy-safe guardrail decisions."
        actions={
          <Button variant="outline" size="sm" asChild>
            <Link to="/guardrails">
              <AppIcon icon={Settings02Icon} data-icon="inline-start" aria-hidden />
              Configure
            </Link>
          </Button>
        }
      />
      <GuardrailsDashboard
        policies={policies}
        decisions={decisions}
        search={search}
        pending={pending}
        onSearch={navigate}
        onPage={(page) =>
          navigate({ ...normalizeFilterValues(search), page, page_size: decisions.page_size })
        }
      />
    </div>
  )
}

function normalizeFilterValues(search: Record<string, unknown>): GuardrailDecisionFiltersInput {
  const filters: GuardrailDecisionFiltersInput = {}
  for (const key of [
    'request_id',
    'phase',
    'action',
    'evaluator',
    'occurred_at_start',
    'occurred_at_end',
  ] as const) {
    const value = search[key]
    if (typeof value === 'string' && value.trim()) filters[key] = value.trim()
  }
  return filters
}

function normalizeSearch(search: Record<string, unknown>): GuardrailDecisionFiltersInput {
  const filters = normalizeFilterValues(search)
  for (const key of ['page', 'page_size'] as const) {
    const value = search[key]
    if (typeof value === 'number' && Number.isInteger(value) && value > 0) filters[key] = value
  }
  return filters
}
