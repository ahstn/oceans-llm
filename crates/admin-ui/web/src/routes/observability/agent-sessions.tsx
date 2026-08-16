import { useEffect, useState, useTransition } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'

import {
  ClearFiltersButton,
  SessionFilters,
} from '@/components/observability/agent-sessions/session-filters'
import {
  DetailSkeleton,
  SessionDetail,
} from '@/components/observability/agent-sessions/session-detail'
import { SessionTable } from '@/components/observability/agent-sessions/session-table'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { getAgentSessions, getObservabilityAgentSessionDetail } from '@/server/admin-data.functions'
import type {
  AgentSessionDetailView,
  AgentSessionFiltersInput,
  AgentSessionSummaryView,
} from '@/types/api'

type AgentSessionRouteSearch = AgentSessionFiltersInput & { session_id?: string }

export const Route = createFileRoute('/observability/agent-sessions')({
  validateSearch: (search: Record<string, unknown>) => normalizeSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => {
    const { session_id: _sessionId, ...filters } = deps
    return getAgentSessions({ data: filters })
  },
  component: AgentSessionsPage,
})

const sessionFilterFields = [
  'lifecycle',
  'gateway_outcome',
  'score_maturity',
  'score_confidence',
  'minimum_coverage_percent',
  'harness_key',
  'requested_model_key',
  'operation',
  'caller_class',
  'user_id',
  'team_id',
  'service_account_id',
  'session_source_id',
  'session_source_hash',
  'request_tag_key',
  'request_tag_value',
] as const satisfies readonly (keyof AgentSessionFiltersInput)[]

export function AgentSessionsPage() {
  const { data: sessionPage } = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const search = Route.useSearch()
  const router = useRouter()
  const [isListPending, startListTransition] = useTransition()
  const selectedSessionId = search.session_id ?? null
  const [selectedDetail, setSelectedDetail] = useState<AgentSessionDetailView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [detailRetry, setDetailRetry] = useState(0)
  const showScore = session.capabilities.calibrated_score_visible

  useEffect(() => {
    if (!selectedSessionId) {
      setSelectedDetail(null)
      setDetailPending(false)
      setDetailError(null)
      return
    }
    let cancelled = false
    setSelectedDetail(null)
    setDetailPending(true)
    setDetailError(null)
    void getObservabilityAgentSessionDetail({ data: { sessionId: selectedSessionId } })
      .then((response) => {
        if (!cancelled) setSelectedDetail(response.data)
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDetailError(error instanceof Error ? error.message : 'Failed to load agent session')
        }
      })
      .finally(() => {
        if (!cancelled) setDetailPending(false)
      })
    return () => {
      cancelled = true
    }
  }, [selectedSessionId, detailRetry])

  function navigate(next: AgentSessionRouteSearch) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/agent-sessions',
        search: normalizeSearch(next as Record<string, unknown>),
      })
    })
  }

  function updateFilter(key: keyof AgentSessionFiltersInput, value: string | undefined) {
    navigate({ ...search, page: 1, [key]: value })
  }

  function openDetail(session: AgentSessionSummaryView) {
    navigate({ ...search, session_id: session.session_id })
  }

  const hasUnavailableCallMetrics = sessionPage.items.some(
    (item) =>
      item.report_schema_version != null &&
      (item.tool_call_count == null || item.mcp_call_count == null),
  )

  return (
    <main className="flex min-w-0 flex-1 flex-col gap-6">
      <header className="flex flex-col gap-2">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <p className="text-muted-foreground text-sm">Observability</p>
            <h1 className="text-2xl font-semibold tracking-tight">Agent sessions</h1>
          </div>
          <Badge variant="outline" className="tabular-nums">
            {sessionPage.total.toLocaleString()} {sessionPage.total === 1 ? 'session' : 'sessions'}
          </Badge>
        </div>
        <p className="text-muted-foreground max-w-3xl text-sm">
          Review the outcome, cost, active time, and data quality for each agent session. The system
          does not show scores until calibration is complete.
        </p>
      </header>

      <Card>
        <CardHeader className="gap-1 border-b">
          <CardTitle className="text-base">Session explorer</CardTitle>
          <CardDescription>
            Filter sessions by owner, harness, score confidence, state, or start date.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4 pt-5">
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <SessionFilters search={search} onChange={updateFilter} />
            </div>
            <ClearFiltersButton
              visible={hasActiveSearch(search)}
              onClear={() => navigate({ page: 1, page_size: 50 })}
            />
          </div>

          {hasUnavailableCallMetrics ? (
            <Alert>
              <AlertTitle>Tool-call data is not available</AlertTitle>
              <AlertDescription>
                The API did not return tool-call or MCP-call counts for some analyzed sessions.
                Restart the gateway after you update it. For local demo data, run{' '}
                <code className="font-mono">mise run gateway-reset-local-demo</code>.
              </AlertDescription>
            </Alert>
          ) : null}

          <SessionTable
            items={sessionPage.items}
            total={sessionPage.total}
            page={search.page ?? 1}
            pageSize={search.page_size ?? 50}
            showScore={showScore}
            loading={isListPending}
            onOpen={openDetail}
            onPageChange={(page, pageSize) => navigate({ ...search, page, page_size: pageSize })}
          />
        </CardContent>
      </Card>

      <Sheet
        open={selectedSessionId !== null}
        onOpenChange={(open) => {
          if (!open) {
            navigate({ ...search, session_id: undefined })
          }
        }}
      >
        <SheetContent className="overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-3xl data-[side=right]:lg:w-[50vw] data-[side=right]:lg:max-w-none data-[side=right]:lg:min-w-[50vw]">
          <SheetHeader className="border-b">
            <SheetTitle>Agent session details</SheetTitle>
            <SheetDescription className="font-mono text-xs">{selectedSessionId}</SheetDescription>
          </SheetHeader>
          <div className="pb-6">
            {detailPending ? <DetailSkeleton /> : null}
            {detailError ? (
              <Alert variant="destructive">
                <AlertTitle>Session details are not available</AlertTitle>
                <AlertDescription className="flex items-center justify-between gap-3">
                  <span>{detailError}</span>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setDetailRetry((value) => value + 1)}
                  >
                    Retry
                  </Button>
                </AlertDescription>
              </Alert>
            ) : null}
            {selectedDetail ? (
              <SessionDetail
                key={selectedDetail.session.session_id}
                detail={selectedDetail}
                showScore={showScore}
                canAccessRequestLogs={session.capabilities.platform_admin}
              />
            ) : null}
          </div>
        </SheetContent>
      </Sheet>
    </main>
  )
}

function normalizeSearch(search: Record<string, unknown>): AgentSessionRouteSearch {
  const result: AgentSessionRouteSearch = {
    page: positiveInteger(search.page, 1),
    page_size: positiveInteger(search.page_size, 50, 100),
  }
  for (const field of [
    'user_id',
    'team_id',
    'service_account_id',
    'harness_key',
    'requested_model_key',
    'operation',
    'caller_class',
    'gateway_outcome',
    'score_maturity',
    'score_confidence',
    'session_source_id',
    'session_source_hash',
    'request_tag_key',
    'request_tag_value',
    'lifecycle',
    'started_after',
    'started_before',
    'session_id',
  ] as const) {
    const value = search[field]
    if (typeof value === 'string' && value.trim()) {
      result[field] = value.trim()
    }
  }
  const minimumCoveragePercent = integerInRange(search.minimum_coverage_percent, 0, 100)
  if (minimumCoveragePercent !== undefined) {
    result.minimum_coverage_percent = minimumCoveragePercent
  }
  return result
}

function positiveInteger(value: unknown, fallback: number, maximum = Number.MAX_SAFE_INTEGER) {
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? Math.min(parsed, maximum) : fallback
}

function integerInRange(value: unknown, minimum: number, maximum: number) {
  if (value === null || value === undefined || value === '') return undefined
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isInteger(parsed) && parsed >= minimum && parsed <= maximum ? parsed : undefined
}

function hasActiveSearch(search: AgentSessionFiltersInput) {
  return Boolean(
    sessionFilterFields.some((field) => search[field] !== null && search[field] !== undefined) ||
    search.started_after ||
    search.started_before,
  )
}
