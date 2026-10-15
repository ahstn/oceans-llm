import { useTransition } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'

import { AuditLogTable } from '@/components/audit/audit-log-table'
import { PageHeader } from '@/components/layout/page-header'
import { getAuditLogPage } from '@/server/admin-data.functions'
import type { AuditLogFiltersInput } from '@/types/api'

export const Route = createFileRoute('/observability/audit-log')({
  validateSearch: (search: Record<string, unknown>) => normalizeSearch(search),
  loaderDeps: ({ search }) => search,
  loader: async ({ deps }) => ({
    page: (await getAuditLogPage({ data: deps })).data,
  }),
  component: AuditLogPage,
})

export function AuditLogPage() {
  const { page } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [pending, startTransition] = useTransition()

  const navigate = (next: AuditLogFiltersInput) =>
    startTransition(async () => {
      await router.navigate({ to: '/observability/audit-log', search: next })
    })

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Audit Log"
        description="Review administrator changes to keys, identity, budgets, sessions and MCP access."
      />
      <AuditLogTable page={page} search={search} pending={pending} onSearch={navigate} />
    </div>
  )
}

const STRING_FILTERS = [
  'actor_user_id',
  'action',
  'target_type',
  'target_id',
  'occurred_at_start',
  'occurred_at_end',
  'cursor',
] as const

function normalizeSearch(search: Record<string, unknown>): AuditLogFiltersInput {
  const filters: AuditLogFiltersInput = {}
  for (const key of STRING_FILTERS) {
    const value = search[key]
    if (typeof value === 'string' && value.trim()) filters[key] = value.trim()
  }
  const limit = search.limit
  if (typeof limit === 'number' && Number.isInteger(limit) && limit > 0) filters.limit = limit
  return filters
}
