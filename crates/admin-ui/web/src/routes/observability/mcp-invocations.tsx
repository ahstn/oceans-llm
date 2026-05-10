import { useEffect, useState, useTransition, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
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
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  getMcpInvocations,
  getObservabilityMcpInvocationDetail,
} from '@/server/admin-data.functions'
import type {
  McpInvocationDetailView,
  McpInvocationFiltersInput,
  McpInvocationStatus,
  McpInvocationView,
} from '@/types/api'

export const Route = createFileRoute('/observability/mcp-invocations')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeFilterSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getMcpInvocations({ data: deps }),
  component: McpInvocationsPage,
})

const initialFilters: McpInvocationFiltersInput = {
  request_id: '',
  server_display_key: '',
  tool_display_key: '',
  api_key_id: '',
  user_id: '',
  team_id: '',
  status: undefined,
  policy_result: undefined,
  occurred_at_start: '',
  occurred_at_end: '',
}

export function McpInvocationsPage() {
  const { data: invocationPage } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [filters, setFilters] = useState<McpInvocationFiltersInput>(() => ({
    ...initialFilters,
    ...search,
  }))
  const [selectedInvocationId, setSelectedInvocationId] = useState<string | null>(null)
  const [selectedDetail, setSelectedDetail] = useState<McpInvocationDetailView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [isListPending, startListTransition] = useTransition()

  useEffect(() => {
    setFilters({ ...initialFilters, ...search })
  }, [search])

  useEffect(() => {
    if (!selectedInvocationId) {
      setSelectedDetail(null)
      setDetailPending(false)
      setDetailError(null)
      return
    }

    let cancelled = false
    setDetailPending(true)
    setDetailError(null)

    void getObservabilityMcpInvocationDetail({ data: { invocationId: selectedInvocationId } })
      .then((response) => {
        if (!cancelled) {
          setSelectedDetail(response.data)
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDetailError(
            error instanceof Error ? error.message : 'Failed to load MCP invocation detail',
          )
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDetailPending(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [selectedInvocationId])

  function updateFilter(key: keyof McpInvocationFiltersInput, value: string) {
    setFilters((current) => ({ ...current, [key]: value }))
  }

  function applyFilters(nextFilters: McpInvocationFiltersInput) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/mcp-invocations',
        search: normalizeFilterSearch(nextFilters),
      })
    })
  }

  const normalizedFilters = normalizeFilterSearch(filters)

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-col gap-1">
          <CardTitle>MCP Invocations</CardTitle>
          <CardDescription>
            Audit request-linked MCP tool calls by API key, user, team, server, tool, policy result,
            status, and payload retention state.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FieldGroup className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
            <Field>
              <FieldLabel htmlFor="mcp-filter-request-id">Request ID</FieldLabel>
              <Input
                id="mcp-filter-request-id"
                data-testid="mcp-filter-request-id"
                placeholder="req_..."
                value={filters.request_id ?? ''}
                onChange={(event) => updateFilter('request_id', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-server">Server</FieldLabel>
              <Input
                id="mcp-filter-server"
                data-testid="mcp-filter-server"
                placeholder="github"
                value={filters.server_display_key ?? ''}
                onChange={(event) => updateFilter('server_display_key', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-tool">Tool</FieldLabel>
              <Input
                id="mcp-filter-tool"
                data-testid="mcp-filter-tool"
                placeholder="create_issue"
                value={filters.tool_display_key ?? ''}
                onChange={(event) => updateFilter('tool_display_key', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-api-key">API Key</FieldLabel>
              <Input
                id="mcp-filter-api-key"
                data-testid="mcp-filter-api-key"
                placeholder="api_key_..."
                value={filters.api_key_id ?? ''}
                onChange={(event) => updateFilter('api_key_id', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-user">User</FieldLabel>
              <Input
                id="mcp-filter-user"
                data-testid="mcp-filter-user"
                placeholder="user_..."
                value={filters.user_id ?? ''}
                onChange={(event) => updateFilter('user_id', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-team">Team</FieldLabel>
              <Input
                id="mcp-filter-team"
                data-testid="mcp-filter-team"
                placeholder="team_..."
                value={filters.team_id ?? ''}
                onChange={(event) => updateFilter('team_id', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel>Status</FieldLabel>
              <Select
                value={filters.status ?? 'all'}
                onValueChange={(value) =>
                  setFilters((current) => ({
                    ...current,
                    status: value === 'all' ? undefined : (value as McpInvocationStatus),
                  }))
                }
              >
                <SelectTrigger data-testid="mcp-filter-status" className="w-full">
                  <SelectValue placeholder="All statuses" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="all">All statuses</SelectItem>
                    <SelectItem value="success">Success</SelectItem>
                    <SelectItem value="unauthorized">Unauthorized</SelectItem>
                    <SelectItem value="policy_denied">Policy denied</SelectItem>
                    <SelectItem value="upstream_error">Upstream error</SelectItem>
                    <SelectItem value="gateway_error">Gateway error</SelectItem>
                    <SelectItem value="timeout">Timeout</SelectItem>
                    <SelectItem value="invalid_request">Invalid request</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel>Policy</FieldLabel>
              <Select
                value={filters.policy_result ?? 'all'}
                onValueChange={(value) =>
                  setFilters((current) => ({
                    ...current,
                    policy_result: value === 'all' ? undefined : value,
                  }))
                }
              >
                <SelectTrigger data-testid="mcp-filter-policy" className="w-full">
                  <SelectValue placeholder="All policy results" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="all">All policies</SelectItem>
                    <SelectItem value="allowed">Allowed</SelectItem>
                    <SelectItem value="denied">Denied</SelectItem>
                    <SelectItem value="not_evaluated">Not evaluated</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-from">From</FieldLabel>
              <Input
                id="mcp-filter-from"
                data-testid="mcp-filter-from"
                placeholder="2026-05-10T00:00:00Z"
                value={filters.occurred_at_start ?? ''}
                onChange={(event) => updateFilter('occurred_at_start', event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="mcp-filter-to">To</FieldLabel>
              <Input
                id="mcp-filter-to"
                data-testid="mcp-filter-to"
                placeholder="2026-05-11T00:00:00Z"
                value={filters.occurred_at_end ?? ''}
                onChange={(event) => updateFilter('occurred_at_end', event.target.value)}
              />
            </Field>
          </FieldGroup>

          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="secondary"
              onClick={() => applyFilters(normalizedFilters)}
              disabled={isListPending}
            >
              {isListPending ? 'Filtering...' : 'Apply Filters'}
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                setFilters(initialFilters)
                applyFilters(initialFilters)
              }}
              disabled={isListPending}
            >
              Clear
            </Button>
          </div>

          <div className="text-sm text-[var(--color-text-soft)]">
            {invocationPage.total} MCP invocation records loaded from gateway observability APIs.
          </div>

          {invocationPage.items.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyTitle>No MCP invocations found</EmptyTitle>
                <EmptyDescription>
                  No tool calls match the current filters or the gateway has not recorded MCP
                  invocation rows yet.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <div className="overflow-x-auto rounded-md border border-[color:var(--color-border)]">
              <Table data-testid="mcp-invocations-table" className="min-w-[78rem]">
                <TableHeader>
                  <TableRow>
                    <TableHead>Request</TableHead>
                    <TableHead>Owner Context</TableHead>
                    <TableHead>Server</TableHead>
                    <TableHead>Tool</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Policy</TableHead>
                    <TableHead>Latency</TableHead>
                    <TableHead>Payload</TableHead>
                    <TableHead>Inspect</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {invocationPage.items.map((item) => (
                    <TableRow key={item.mcp_tool_invocation_id}>
                      <TableCell>
                        <div className="flex flex-col gap-1">
                          <span className="font-mono text-xs">{item.request_id ?? 'n/a'}</span>
                          <span className="font-mono text-xs text-[var(--color-text-soft)]">
                            {item.mcp_tool_invocation_id}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell>
                        <OwnerLabel item={item} />
                      </TableCell>
                      <TableCell className="font-mono">{item.server_display_key}</TableCell>
                      <TableCell className="font-mono">{item.tool_display_key}</TableCell>
                      <TableCell>
                        <div className="flex flex-col gap-1">
                          <Badge variant={statusBadgeVariant(item.status)}>
                            {formatStatus(item.status)}
                          </Badge>
                          {item.error_code ? (
                            <span className="text-xs text-[var(--color-text-soft)]">
                              {item.error_code}
                            </span>
                          ) : null}
                        </div>
                      </TableCell>
                      <TableCell>
                        <Badge variant={policyBadgeVariant(item.policy_result)}>
                          {formatPolicyResult(item.policy_result)}
                        </Badge>
                      </TableCell>
                      <TableCell>{formatLatency(item.latency_ms)}</TableCell>
                      <TableCell>
                        <PayloadBadges item={item} />
                      </TableCell>
                      <TableCell>
                        <Button
                          type="button"
                          variant="secondary"
                          onClick={() => setSelectedInvocationId(item.mcp_tool_invocation_id)}
                        >
                          Inspect
                        </Button>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>

      {selectedInvocationId ? (
        <Card data-testid="mcp-invocation-detail">
          <CardHeader className="flex flex-row items-start justify-between gap-4">
            <div className="flex flex-col gap-1">
              <CardTitle>MCP Invocation Detail</CardTitle>
              <CardDescription>
                Review owner context, authorization result, sanitized arguments, and sanitized
                result metadata.
              </CardDescription>
            </div>
            <Button type="button" variant="ghost" onClick={() => setSelectedInvocationId(null)}>
              Close
            </Button>
          </CardHeader>
          <CardContent>
            {detailPending ? (
              <DetailSkeleton />
            ) : detailError ? (
              <Alert variant="destructive">
                <AlertTitle>MCP invocation detail failed</AlertTitle>
                <AlertDescription>{detailError}</AlertDescription>
              </Alert>
            ) : selectedDetail ? (
              <InvocationDetail detail={selectedDetail} />
            ) : (
              <DetailSkeleton />
            )}
          </CardContent>
        </Card>
      ) : null}
    </div>
  )
}

function InvocationDetail({ detail }: { detail: McpInvocationDetailView }) {
  const { invocation } = detail

  return (
    <div className="grid gap-4">
      <div className="grid gap-3 rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4 md:grid-cols-2 lg:grid-cols-3">
        <DetailRow label="Invocation ID" value={invocation.mcp_tool_invocation_id} mono />
        <DetailRow label="Request ID" value={invocation.request_id ?? 'n/a'} mono />
        <DetailRow label="Owner" value={<OwnerLabel item={invocation} />} />
        <DetailRow label="API Key" value={invocation.api_key_id ?? 'n/a'} mono />
        <DetailRow label="User" value={invocation.user_id ?? 'n/a'} mono />
        <DetailRow label="Team" value={invocation.team_id ?? 'n/a'} mono />
        <DetailRow label="Server" value={invocation.server_display_key} mono />
        <DetailRow label="Tool" value={invocation.tool_display_key} mono />
        <DetailRow label="Occurred At" value={invocation.occurred_at} />
        <DetailRow label="Latency" value={formatLatency(invocation.latency_ms)} />
        <DetailRow
          label="Status"
          value={
            <Badge variant={statusBadgeVariant(invocation.status)}>
              {formatStatus(invocation.status)}
            </Badge>
          }
        />
        <DetailRow
          label="Policy"
          value={
            <Badge variant={policyBadgeVariant(invocation.policy_result)}>
              {formatPolicyResult(invocation.policy_result)}
            </Badge>
          }
        />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Payload State</CardTitle>
          <CardDescription>
            MCP arguments and results are stored only after redaction and bounded truncation.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-3 text-sm md:grid-cols-3">
            <DetailRow label="Payload Stored" value={invocation.has_payload ? 'yes' : 'no'} />
            <DetailRow
              label="Arguments"
              value={payloadState(
                invocation.arguments_payload_redacted,
                invocation.arguments_payload_truncated,
              )}
            />
            <DetailRow
              label="Result"
              value={payloadState(
                invocation.result_payload_redacted,
                invocation.result_payload_truncated,
              )}
            />
          </dl>
        </CardContent>
      </Card>

      <div className="grid gap-4 lg:grid-cols-2">
        <PayloadCard
          title="Arguments"
          truncated={invocation.arguments_payload_truncated}
          payload={detail.payload?.arguments_json}
        />
        <PayloadCard
          title="Result"
          truncated={invocation.result_payload_truncated}
          payload={detail.payload?.result_json}
        />
      </div>
    </div>
  )
}

function DetailRow({
  label,
  value,
  mono = false,
}: {
  label: string
  value: ReactNode
  mono?: boolean
}) {
  return (
    <div>
      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
        {label}
      </dt>
      <dd
        className={
          mono ? 'font-mono text-sm text-[var(--color-text)]' : 'text-sm text-[var(--color-text)]'
        }
      >
        {value}
      </dd>
    </div>
  )
}

function DetailSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-24 w-full" />
      <Skeleton className="h-32 w-full" />
      <Skeleton className="h-48 w-full" />
    </div>
  )
}

function PayloadCard({
  title,
  truncated,
  payload,
}: {
  title: string
  truncated: boolean
  payload: unknown
}) {
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex flex-col gap-1">
            <CardTitle>{title}</CardTitle>
            <CardDescription>
              {truncated
                ? 'Sanitized MCP payload was truncated before persistence.'
                : 'Sanitized MCP payload.'}
            </CardDescription>
          </div>
          {truncated ? (
            <Badge variant="warning">truncated</Badge>
          ) : (
            <Badge variant="outline">full</Badge>
          )}
        </div>
      </CardHeader>
      <CardContent>
        {payload !== null && payload !== undefined ? (
          <pre className="max-h-[360px] overflow-auto text-xs leading-6 text-[var(--color-text-muted)]">
            {JSON.stringify(payload, null, 2)}
          </pre>
        ) : (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No payload stored</EmptyTitle>
              <EmptyDescription>
                Payload capture was summary-only or unavailable for this invocation.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
      </CardContent>
    </Card>
  )
}

function OwnerLabel({ item }: { item: McpInvocationView }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="font-medium">
        {item.owner_kind}: {item.team_id ?? item.user_id ?? item.api_key_id ?? 'n/a'}
      </span>
      <span className="font-mono text-xs text-[var(--color-text-soft)]">
        api:{item.api_key_id ?? 'n/a'} user:{item.user_id ?? 'n/a'} team:{item.team_id ?? 'n/a'}
      </span>
    </div>
  )
}

function PayloadBadges({ item }: { item: McpInvocationView }) {
  return (
    <span className="inline-flex flex-wrap gap-1">
      <Badge variant={item.has_payload ? 'secondary' : 'outline'}>
        {item.has_payload ? 'payload' : 'summary only'}
      </Badge>
      {item.arguments_payload_redacted ? <Badge variant="outline">args redacted</Badge> : null}
      {item.result_payload_redacted ? <Badge variant="outline">result redacted</Badge> : null}
      {item.arguments_payload_truncated ? <Badge variant="warning">args truncated</Badge> : null}
      {item.result_payload_truncated ? <Badge variant="warning">result truncated</Badge> : null}
    </span>
  )
}

function statusBadgeVariant(status: McpInvocationStatus): 'success' | 'warning' | 'destructive' {
  return status === 'success'
    ? 'success'
    : status === 'unauthorized' || status === 'policy_denied'
      ? 'destructive'
      : 'warning'
}

function policyBadgeVariant(policyResult: string): 'success' | 'warning' | 'outline' {
  return policyResult === 'allowed' ? 'success' : policyResult === 'denied' ? 'warning' : 'outline'
}

function formatStatus(status: string) {
  return status.replaceAll('_', ' ')
}

function formatPolicyResult(policyResult: string) {
  return policyResult.replaceAll('_', ' ')
}

function formatLatency(latencyMs: number | null) {
  return latencyMs === null ? 'n/a' : `${latencyMs}ms`
}

function payloadState(redacted: boolean, truncated: boolean) {
  if (redacted && truncated) {
    return 'redacted, truncated'
  }
  if (redacted) {
    return 'redacted'
  }
  if (truncated) {
    return 'truncated'
  }
  return 'stored'
}

function normalizeFilterSearch(search: Record<string, unknown>): McpInvocationFiltersInput {
  const status = searchParamValue(search.status)
  return {
    request_id: searchParamValue(search.request_id),
    server_display_key: searchParamValue(search.server_display_key),
    tool_display_key: searchParamValue(search.tool_display_key),
    api_key_id: searchParamValue(search.api_key_id),
    user_id: searchParamValue(search.user_id),
    team_id: searchParamValue(search.team_id),
    status: isMcpInvocationStatus(status) ? status : undefined,
    policy_result: searchParamValue(search.policy_result),
    occurred_at_start: searchParamValue(search.occurred_at_start),
    occurred_at_end: searchParamValue(search.occurred_at_end),
  }
}

function searchParamValue(value: unknown): string | undefined {
  if (typeof value !== 'string') {
    return undefined
  }

  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

function isMcpInvocationStatus(value: string | undefined): value is McpInvocationStatus {
  return (
    value === 'success' ||
    value === 'unauthorized' ||
    value === 'policy_denied' ||
    value === 'upstream_error' ||
    value === 'gateway_error' ||
    value === 'timeout' ||
    value === 'invalid_request'
  )
}
