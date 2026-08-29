import { useEffect, useState, useTransition } from 'react'
import { createFileRoute, useRouter, useRouterState } from '@tanstack/react-router'

import { PageHeader } from '@/components/layout/page-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from '@/components/ui/pagination'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { getGuardrailDecisionPage, getGuardrailPolicyView } from '@/server/admin-data.functions'
import type { GuardrailDecisionFiltersInput, GuardrailPoliciesView } from '@/types/api'

const timestampFormatter = new Intl.DateTimeFormat('en-US', {
  dateStyle: 'medium',
  timeStyle: 'medium',
  timeZone: 'UTC',
})

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

const emptyFilters: GuardrailDecisionFiltersInput = {
  request_id: '',
  phase: '',
  action: '',
  evaluator: '',
  occurred_at_start: '',
  occurred_at_end: '',
}

export function GuardrailsPage() {
  const { policies, decisions } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const routeSearch = useRouterState({ select: (state) => state.location.searchStr })
  const [filters, setFilters] = useState<GuardrailDecisionFiltersInput>({
    ...emptyFilters,
    ...normalizeFilterValues(search),
  })
  const [pending, startTransition] = useTransition()

  useEffect(() => {
    setFilters({
      ...emptyFilters,
      ...normalizeFilterValues(Object.fromEntries(new URLSearchParams(routeSearch))),
    })
  }, [routeSearch])

  function applyFilters() {
    startTransition(async () => {
      await router.navigate({
        to: '/observability/guardrails',
        search: normalizeFilterValues(filters),
      })
    })
  }

  function goToPage(page: number) {
    startTransition(async () => {
      await router.navigate({
        to: '/observability/guardrails',
        search: {
          ...normalizeFilterValues(search),
          page,
          page_size: decisions.page_size,
        },
      })
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Guardrails"
        description="Review effective read-only policies and privacy-safe guardrail decisions."
      />

      <Card>
        <CardHeader>
          <CardTitle>Effective policies</CardTitle>
          <CardDescription>
            Configuration is authoritative. This view cannot change policy.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4 lg:grid-cols-3">
          <PolicyCard name="Global default" policy={policies.default} />
          {Object.entries(policies.model_routes).map(([name, policy]) => (
            <PolicyCard key={`model:${name}`} name={`Model: ${name}`} policy={policy} />
          ))}
          {Object.entries(policies.mcp_servers).map(([name, policy]) => (
            <PolicyCard key={`mcp:${name}`} name={`MCP: ${name}`} policy={policy} />
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Decision events</CardTitle>
          <CardDescription>
            Raw prompts, commands, arguments, and results are not shown.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FieldGroup className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
            <Field>
              <FieldLabel htmlFor="guardrail-request-id">Request ID</FieldLabel>
              <Input
                id="guardrail-request-id"
                value={filters.request_id ?? ''}
                onChange={(event) =>
                  setFilters((current) => ({ ...current, request_id: event.target.value }))
                }
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="guardrail-evaluator">Evaluator</FieldLabel>
              <Input
                id="guardrail-evaluator"
                value={filters.evaluator ?? ''}
                onChange={(event) =>
                  setFilters((current) => ({ ...current, evaluator: event.target.value }))
                }
              />
            </Field>
            <Field>
              <FieldLabel>Phase</FieldLabel>
              <Select
                value={filters.phase || 'all'}
                onValueChange={(value) =>
                  setFilters((current) => ({ ...current, phase: value === 'all' ? '' : value }))
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="All phases" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="all">All phases</SelectItem>
                    <SelectItem value="prompt">Prompt</SelectItem>
                    <SelectItem value="model_response">Model response</SelectItem>
                    <SelectItem value="generated_tool_call">Generated tool call</SelectItem>
                    <SelectItem value="mcp_call">MCP call</SelectItem>
                    <SelectItem value="mcp_result">MCP result</SelectItem>
                    <SelectItem value="harness_pre_tool">Harness pre-tool</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel>Action</FieldLabel>
              <Select
                value={filters.action || 'all'}
                onValueChange={(value) =>
                  setFilters((current) => ({ ...current, action: value === 'all' ? '' : value }))
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="All actions" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="all">All actions</SelectItem>
                    <SelectItem value="allow">Allow</SelectItem>
                    <SelectItem value="audit">Audit</SelectItem>
                    <SelectItem value="deny">Deny</SelectItem>
                    <SelectItem value="transformed">Transformed</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
          </FieldGroup>
          <div className="flex gap-2">
            <Button disabled={pending} onClick={applyFilters}>
              Apply filters
            </Button>
            <Button
              variant="outline"
              disabled={pending}
              onClick={() => {
                setFilters(emptyFilters)
                startTransition(async () => {
                  await router.navigate({ to: '/observability/guardrails', search: {} })
                })
              }}
            >
              Clear
            </Button>
          </div>

          {decisions.items.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyTitle>No guardrail decisions</EmptyTitle>
                <EmptyDescription>No events match the current filters.</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Time</TableHead>
                  <TableHead>Phase</TableHead>
                  <TableHead>Action</TableHead>
                  <TableHead>Evaluator</TableHead>
                  <TableHead>Rule</TableHead>
                  <TableHead>Reason</TableHead>
                  <TableHead>Latency</TableHead>
                  <TableHead>Decision ID</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {decisions.items.map((decision) => (
                  <TableRow key={decision.decision_id}>
                    <TableCell>
                      {timestampFormatter.format(new Date(decision.occurred_at))}
                    </TableCell>
                    <TableCell>{decision.phase}</TableCell>
                    <TableCell>
                      <Badge variant={decision.action === 'deny' ? 'destructive' : 'secondary'}>
                        {decision.action}
                      </Badge>
                    </TableCell>
                    <TableCell>{decision.managed_service ?? decision.evaluator}</TableCell>
                    <TableCell>
                      {[decision.pack_id, decision.rule_id].filter(Boolean).join(' / ') || '—'}
                    </TableCell>
                    <TableCell>{decision.reason_code}</TableCell>
                    <TableCell>{formatLatency(decision.latency_micros)}</TableCell>
                    <TableCell className="font-mono text-xs">{decision.decision_id}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
          <p className="text-muted-foreground text-sm">
            Showing {decisions.items.length} of {decisions.total} decisions.
          </p>
          {decisions.total > decisions.page_size ? (
            <Pagination className="mx-0 w-auto justify-end">
              <PaginationContent>
                {decisions.page > 1 ? (
                  <PaginationItem>
                    <PaginationPrevious
                      href="#"
                      onClick={(event) => {
                        event.preventDefault()
                        goToPage(decisions.page - 1)
                      }}
                    />
                  </PaginationItem>
                ) : null}
                <PaginationItem>
                  <PaginationLink href="#" isActive onClick={(event) => event.preventDefault()}>
                    {decisions.page}
                  </PaginationLink>
                </PaginationItem>
                {decisions.page * decisions.page_size < decisions.total ? (
                  <PaginationItem>
                    <PaginationNext
                      href="#"
                      onClick={(event) => {
                        event.preventDefault()
                        goToPage(decisions.page + 1)
                      }}
                    />
                  </PaginationItem>
                ) : null}
              </PaginationContent>
            </Pagination>
          ) : null}
        </CardContent>
      </Card>
    </div>
  )
}

function PolicyCard({ name, policy }: { name: string; policy: GuardrailPoliciesView['default'] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{name}</CardTitle>
        <CardDescription>{policy.scope}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 text-sm">
        <div className="flex gap-2">
          <Badge variant={policy.enabled ? 'secondary' : 'outline'}>
            {policy.enabled ? 'Enabled' : 'Disabled'}
          </Badge>
          <Badge variant="outline">{policy.mode}</Badge>
        </div>
        <p>
          <span className="font-medium">Packs:</span> {policy.packs.join(', ') || 'None'}
        </p>
        <p>
          <span className="font-medium">Managed checks:</span>{' '}
          {policy.managed_checks.join(', ') || 'None'}
        </p>
        <p>
          <span className="font-medium">Stream buffer:</span>{' '}
          {policy.stream_buffer_bytes.toLocaleString()} bytes
        </p>
      </CardContent>
    </Card>
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

function formatLatency(micros: number) {
  return micros >= 1_000 ? `${(micros / 1_000).toFixed(1)} ms` : `${micros} µs`
}
