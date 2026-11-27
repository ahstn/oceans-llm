import { useEffect, useState, useTransition, type KeyboardEvent, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import {
  ApiIcon,
  Calendar04Icon,
  CalendarAdd01Icon,
  CloudServerIcon,
  HashIcon,
  Key01Icon,
  Shield01Icon,
  StatusIcon,
  Target02Icon,
  UserGroupIcon,
  UserIcon,
} from '@hugeicons/core-free-icons'

import {
  clearedFilters,
  dayPicker,
  enumOptions,
  strictValue,
  type FilterGroup,
  type FilterPatch,
} from '@/components/data/filter-fields'
import {
  anyTypedValue,
  useLoadedFilterOptions,
  type FilterOption,
  type FilterOptionsState,
} from '@/components/data/filter-options'
import { FilterToolbar } from '@/components/data/filter-toolbar'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { PageHeader } from '@/components/layout/page-header'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { EMPTY_VALUE, formatDuration, formatTimestamp } from '@/lib/format'
import {
  getMcpInvocations,
  getMcpServers,
  getObservabilityMcpInvocationDetail,
} from '@/server/admin-data.functions'
import type {
  McpInvocationDetailView,
  McpInvocationFiltersInput,
  McpInvocationStatus,
  McpInvocationView,
} from '@/types/api'

export const Route = createFileRoute('/observability/mcp-invocations')({
  validateSearch: (search: Record<string, unknown>) => normalizeFilterSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getMcpInvocations({ data: deps }),
  component: McpInvocationsPage,
})

type InvocationFilterParam = keyof McpInvocationFiltersInput

const invocationStatuses: McpInvocationStatus[] = [
  'success',
  'unauthorized',
  'policy_denied',
  'upstream_error',
  'gateway_error',
  'timeout',
  'invalid_request',
]

/** Methods the gateway records today; others can still be typed. */
const methodOptions: FilterOption[] = [
  'tools/call',
  'tools/list',
  'resources/read',
  'resources/list',
  'prompts/get',
  'prompts/list',
].map((value) => ({ value, label: value }))

const invocationFilterGroups: FilterGroup<InvocationFilterParam>[] = [
  {
    label: 'Invocation',
    fields: [
      {
        key: 'request_id',
        params: ['request_id'],
        label: 'Request ID',
        icon: HashIcon,
        placeholder: 'request ID',
      },
      {
        key: 'server_display_key',
        params: ['server_display_key'],
        label: 'Server',
        icon: CloudServerIcon,
        placeholder: 'Select a server',
        picker: {
          source: 'servers',
          searchPlaceholder: 'Search or type a server key…',
          customValue: anyTypedValue,
        },
      },
      {
        key: 'tool_display_key',
        params: ['tool_display_key'],
        label: 'Target',
        icon: Target02Icon,
        placeholder: 'tool key',
      },
      {
        key: 'mcp_method',
        params: ['mcp_method'],
        label: 'Method',
        icon: ApiIcon,
        placeholder: 'Select a method',
        picker: {
          source: methodOptions,
          searchPlaceholder: 'Search or type a method…',
          customValue: anyTypedValue,
        },
      },
      {
        key: 'status',
        params: ['status'],
        label: 'Status',
        icon: StatusIcon,
        placeholder: 'Select a status',
        picker: {
          source: enumOptions(invocationStatuses),
          searchPlaceholder: 'Search statuses…',
          customValue: strictValue,
        },
      },
      {
        key: 'policy_result',
        params: ['policy_result'],
        label: 'Policy',
        icon: Shield01Icon,
        placeholder: 'Select a result',
        picker: {
          source: enumOptions(['allowed', 'denied', 'not_evaluated']),
          searchPlaceholder: 'Search policy results…',
          customValue: strictValue,
        },
      },
    ],
  },
  {
    label: 'Owner',
    fields: [
      {
        key: 'api_key_id',
        params: ['api_key_id'],
        label: 'API key ID',
        icon: Key01Icon,
        placeholder: 'API key ID',
      },
      {
        key: 'user_id',
        params: ['user_id'],
        label: 'User ID',
        icon: UserIcon,
        placeholder: 'user ID',
      },
      {
        key: 'team_id',
        params: ['team_id'],
        label: 'Team ID',
        icon: UserGroupIcon,
        placeholder: 'team ID',
      },
    ],
  },
  {
    label: 'Date',
    fields: [
      {
        key: 'occurred_at_start',
        params: ['occurred_at_start'],
        label: 'Occurred after',
        icon: CalendarAdd01Icon,
        placeholder: 'Select a day',
        picker: dayPicker('start'),
      },
      {
        key: 'occurred_at_end',
        params: ['occurred_at_end'],
        label: 'Occurred before',
        icon: Calendar04Icon,
        placeholder: 'Select a day',
        picker: dayPicker('end'),
      },
    ],
  },
]

/** Loads the MCP server catalog once the Server chip shows; retired keys can still be typed. */
function useInvocationCatalogOptions(shows: (source: string) => boolean) {
  const serverOptions = useLoadedFilterOptions(shows('servers'), loadServerOptions)
  return (source: string): FilterOptionsState | undefined =>
    source === 'servers' ? serverOptions : undefined
}

async function loadServerOptions(signal: AbortSignal): Promise<FilterOption[]> {
  const servers = await getMcpServers({ data: { include_disabled: true }, signal })
  return servers.data.items
    .map((server) => ({
      value: server.server_key,
      label: server.display_name,
      hint: server.server_key,
    }))
    .sort((left, right) => left.label.localeCompare(right.label))
}

// Filters, selection, and detail loading share one MCP invocation explorer state.
export function McpInvocationsPage() {
  const { data: invocationPage } = Route.useLoaderData()
  const search = Route.useSearch()
  const router = useRouter()
  const [selectedInvocationId, setSelectedInvocationId] = useState<string | null>(null)
  const [selectedDetail, setSelectedDetail] = useState<McpInvocationDetailView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [isListPending, startListTransition] = useTransition()

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

  // Patches merge into the latest location so two chip commits that land close together cannot
  // overwrite each other with stale filter values.
  function applyFilters(patch: FilterPatch<InvocationFilterParam>) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/mcp-invocations',
        search: (previous) =>
          normalizeFilterSearch({ ...(previous as Record<string, unknown>), ...patch }),
        resetScroll: false,
      })
    })
  }

  const hasActiveFilters = Object.values(search).some((value) => value !== undefined)

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="MCP invocations"
        description="Review each use of an MCP tool, its result, and the data that the system stored."
      />

      <Card>
        <CardHeader className="flex flex-col gap-1">
          <CardTitle>Tool activity</CardTitle>
          <CardDescription>
            Filter the records, then select one to review more details.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FilterToolbar
            groups={invocationFilterGroups}
            filters={search}
            shownCount={invocationPage.items.length}
            totalCount={invocationPage.total}
            isPending={isListPending}
            testIdPrefix="mcp-invocation"
            useCatalogOptions={useInvocationCatalogOptions}
            onApply={applyFilters}
          />

          <InvocationTable
            items={invocationPage.items}
            hasActiveFilters={hasActiveFilters}
            onClearFilters={() => applyFilters(clearedFilters(invocationFilterGroups))}
            onInspect={setSelectedInvocationId}
          />
        </CardContent>
      </Card>

      {selectedInvocationId ? (
        <Card data-testid="mcp-invocation-detail">
          <CardHeader className="flex flex-row items-start justify-between gap-4">
            <div className="flex flex-col gap-1">
              <CardTitle>Invocation detail</CardTitle>
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

function InvocationTable({
  items,
  hasActiveFilters,
  onClearFilters,
  onInspect,
}: {
  items: McpInvocationView[]
  hasActiveFilters: boolean
  onClearFilters: () => void
  onInspect: (invocationId: string) => void
}) {
  if (items.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No MCP invocations found</EmptyTitle>
          <EmptyDescription>
            {hasActiveFilters
              ? 'No MCP calls match the current filters.'
              : 'The gateway records a row here each time an agent calls an MCP tool through it.'}
          </EmptyDescription>
        </EmptyHeader>
        {hasActiveFilters ? (
          <Button type="button" variant="outline" size="sm" onClick={onClearFilters}>
            Clear filters
          </Button>
        ) : null}
      </Empty>
    )
  }

  return (
    <div className="border-border overflow-x-auto rounded-md border">
      <Table data-testid="mcp-invocations-table" className="min-w-[60rem]">
        <TableHeader>
          <TableRow>
            <TableHead>Time</TableHead>
            <TableHead>Server</TableHead>
            <TableHead>Target</TableHead>
            <TableHead>Owner</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Policy</TableHead>
            <TableHead className="text-right">Latency</TableHead>
            <TableHead>Payload</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.map((item) => (
            <InvocationRow key={item.mcp_tool_invocation_id} item={item} onInspect={onInspect} />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

/** One invocation; the whole row opens the detail on click, Enter or Space. */
function InvocationRow({
  item,
  onInspect,
}: {
  item: McpInvocationView
  onInspect: (invocationId: string) => void
}) {
  return (
    <TableRow
      data-invocation-id={item.mcp_tool_invocation_id}
      tabIndex={0}
      aria-label={`Inspect ${item.tool_display_key} on ${item.server_display_key}`}
      className="cursor-pointer"
      onClick={() => onInspect(item.mcp_tool_invocation_id)}
      onKeyDown={(event: KeyboardEvent<HTMLTableRowElement>) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onInspect(item.mcp_tool_invocation_id)
        }
      }}
    >
      <TableCell>
        <div className="flex min-w-0 flex-col">
          <span className="whitespace-nowrap tabular-nums">
            {formatTimestamp(item.occurred_at, { seconds: true })}
          </span>
          <span
            className="text-muted-foreground max-w-48 truncate font-mono text-xs"
            title={item.request_id ?? undefined}
          >
            {item.request_id ?? EMPTY_VALUE}
          </span>
        </div>
      </TableCell>
      <TableCell>
        {/* Capped so a long server key truncates instead of pushing Latency out of the card. */}
        <div className="flex max-w-44 min-w-0 flex-col">
          <span className="truncate" title={item.server_display_name}>
            {item.server_display_name}
          </span>
          <span
            className="text-muted-foreground truncate font-mono text-xs"
            title={item.server_display_key}
          >
            {item.server_display_key}
          </span>
        </div>
      </TableCell>
      <TableCell>
        <div className="flex max-w-44 min-w-0 flex-col">
          <span className="truncate font-mono text-sm" title={item.tool_display_key}>
            {item.tool_display_key}
          </span>
          <span className="text-muted-foreground font-mono text-xs">{item.mcp_method}</span>
        </div>
      </TableCell>
      <TableCell>
        <OwnerLabel item={item} />
      </TableCell>
      <TableCell>
        <div className="flex max-w-32 min-w-0 flex-col gap-1">
          <Badge variant={statusBadgeVariant(item.status)}>{formatEnum(item.status)}</Badge>
          {item.error_code ? (
            <span
              className="text-muted-foreground truncate font-mono text-xs"
              title={item.error_code}
            >
              {item.error_code}
            </span>
          ) : null}
        </div>
      </TableCell>
      <TableCell>
        <Badge variant={policyBadgeVariant(item.policy_result)}>
          {formatEnum(item.policy_result)}
        </Badge>
      </TableCell>
      <TableCell className="text-right whitespace-nowrap tabular-nums">
        {formatDuration(item.latency_ms)}
      </TableCell>
      <TableCell>
        <PayloadBadges item={item} />
      </TableCell>
    </TableRow>
  )
}

function InvocationDetail({ detail }: { detail: McpInvocationDetailView }) {
  const { invocation } = detail

  return (
    <div className="grid gap-4">
      <div className="border-border bg-surface-muted grid gap-3 rounded-md border p-4 md:grid-cols-2 lg:grid-cols-3">
        <DetailRow label="Invocation ID" value={invocation.mcp_tool_invocation_id} mono />
        <DetailRow label="Request ID" value={invocation.request_id ?? EMPTY_VALUE} mono />
        <DetailRow label="Owner" value={<OwnerLabel item={invocation} />} />
        <DetailRow label="API key" value={invocation.api_key_id ?? EMPTY_VALUE} mono />
        <DetailRow label="User" value={invocation.user_id ?? EMPTY_VALUE} mono />
        <DetailRow label="Team" value={invocation.team_id ?? EMPTY_VALUE} mono />
        <DetailRow label="Server" value={invocation.server_display_key} mono />
        <DetailRow label="Method" value={invocation.mcp_method} mono />
        <DetailRow label="Target" value={invocation.tool_display_key} mono />
        <DetailRow
          label="Tool version"
          value={
            invocation.tool_schema_version == null
              ? EMPTY_VALUE
              : String(invocation.tool_schema_version)
          }
          mono
        />
        <DetailRow
          label="Occurred at"
          value={formatTimestamp(invocation.occurred_at, { seconds: true })}
        />
        <DetailRow label="Latency" value={formatDuration(invocation.latency_ms)} />
        <DetailRow
          label="Status"
          value={
            <Badge variant={statusBadgeVariant(invocation.status)}>
              {formatEnum(invocation.status)}
            </Badge>
          }
        />
        <DetailRow
          label="Policy"
          value={
            <Badge variant={policyBadgeVariant(invocation.policy_result)}>
              {formatEnum(invocation.policy_result)}
            </Badge>
          }
        />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Payload state</CardTitle>
          <CardDescription>
            MCP arguments and results are stored only after redaction and bounded truncation.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-3 text-sm md:grid-cols-3">
            <DetailRow label="Payload stored" value={invocation.has_payload ? 'Yes' : 'No'} />
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
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className={mono ? 'text-foreground font-mono text-sm' : 'text-foreground text-sm'}>
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
          <pre className="text-subtle-foreground max-h-[360px] overflow-auto text-xs leading-6">
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

const OWNER_KIND_LABELS: Record<string, string> = {
  team: 'Team',
  user: 'User',
  api_key: 'API key',
  service_account: 'Service account',
}

/** The owner kind and its ID, shortened; every owner ID stays in the title and accessible name. */
function OwnerLabel({ item }: { item: McpInvocationView }) {
  const ownerId =
    item.owner_kind === 'user'
      ? item.user_id
      : item.owner_kind === 'team'
        ? item.team_id
        : item.owner_kind === 'api_key'
          ? item.api_key_id
          : (item.team_id ?? item.user_id ?? item.api_key_id)
  const kind = OWNER_KIND_LABELS[item.owner_kind] ?? formatEnum(item.owner_kind)
  const allIds = [
    item.api_key_id ? `API key ${item.api_key_id}` : null,
    item.user_id ? `user ${item.user_id}` : null,
    item.team_id ? `team ${item.team_id}` : null,
  ]
    .filter(Boolean)
    .join(', ')
  return (
    <div className="flex min-w-0 flex-col" title={allIds || undefined}>
      <span>{kind}</span>
      <span className="text-muted-foreground truncate font-mono text-xs">
        {ownerId ? shortId(ownerId) : EMPTY_VALUE}
      </span>
    </div>
  )
}

function shortId(id: string) {
  return id.length > 12 ? `${id.slice(0, 8)}…` : id
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

/** An API enum in sentence case: `policy_denied` reads `Policy denied`. */
function formatEnum(value: string) {
  const words = value.replaceAll('_', ' ')
  return words.charAt(0).toUpperCase() + words.slice(1)
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
    mcp_method: searchParamValue(search.mcp_method),
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
