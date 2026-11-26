import { useEffect, useState, useTransition } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'

import { ColumnVisibilityPopover } from '@/components/data/column-visibility-popover'
import {
  clearedSessionFilters,
  hasSessionFilters,
  sessionFilterParams,
} from '@/components/observability/agent-sessions/session-filter-fields'
import {
  SessionFilterToolbar,
  type SessionFilterPatch,
} from '@/components/observability/agent-sessions/session-filter-toolbar'
import {
  DetailSkeleton,
  SessionDetail,
} from '@/components/observability/agent-sessions/session-detail'
import {
  defaultVisibleSessionColumns,
  toggleableSessionColumns,
} from '@/components/observability/agent-sessions/session-columns'
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
  // The open session lives in the URL but does not change the list, so it stays out of the
  // deps: opening or closing a session then reuses the loaded list instead of reading it again.
  loaderDeps: ({ search: { session_id: _sessionId, ...filters } }) => filters,
  loader: ({ deps }) => getAgentSessions({ data: deps }),
  component: AgentSessionsPage,
})

// Session filtering, selection, and detail loading form one explorer workflow.
// oxlint-disable-next-line eslint/max-lines-per-function
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
  const [visibleColumns, setVisibleColumns] = useState(defaultVisibleSessionColumns)
  // Remounting the toolbar drops chips that were added but never filled in.
  const [toolbarKey, setToolbarKey] = useState(0)
  const showScore = session?.capabilities.calibrated_score_visible ?? false

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

  function updateRouteSearch(next: AgentSessionRouteSearch) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/agent-sessions',
        search: normalizeSearch(next as Record<string, unknown>),
      })
    })
  }

  // Patches merge into the latest location so two chip commits that land close together cannot
  // overwrite each other with stale filter values. Any filter change returns to the first page.
  function applyFilters(patch: SessionFilterPatch) {
    startListTransition(async () => {
      await router.navigate({
        to: '/observability/agent-sessions',
        search: (previous) =>
          normalizeSearch({ ...(previous as Record<string, unknown>), ...patch, page: 1 }),
        resetScroll: false,
      })
    })
  }

  function clearFilters() {
    applyFilters(clearedSessionFilters)
    setToolbarKey((key) => key + 1)
  }

  function openDetail(session: AgentSessionSummaryView) {
    updateRouteSearch({ ...search, session_id: session.session_id })
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
            Filter sessions, then select one to review its requests, tools, and data quality.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4 pt-5">
          <SessionFilterToolbar
            key={toolbarKey}
            filters={search}
            shownCount={sessionPage.items.length}
            totalCount={sessionPage.total}
            isPending={isListPending}
            onApply={applyFilters}
            actions={
              <div className="hidden md:block">
                <ColumnVisibilityPopover
                  description="Show or hide session table columns. Started always shows."
                  columns={toggleableSessionColumns}
                  visible={visibleColumns}
                  onVisibleChange={(key, visible) =>
                    setVisibleColumns((current) => ({ ...current, [key]: visible }))
                  }
                />
              </div>
            }
          />

          {hasUnavailableCallMetrics ? (
            <Alert>
              <AlertTitle>Tool-call data is not available</AlertTitle>
              <AlertDescription>
                The API did not return tool-call or MCP-call counts for some analyzed sessions.
                Restart the gateway after you update it. For local demo data, run{' '}
                <code className="font-mono">mise run //gateway:reset-local-demo</code>.
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
            visibleColumns={visibleColumns}
            emptyAction={
              hasSessionFilters(search) ? (
                <Button type="button" variant="outline" size="sm" onClick={clearFilters}>
                  Clear filters
                </Button>
              ) : null
            }
            onOpen={openDetail}
            onPageChange={(page, pageSize) =>
              updateRouteSearch({ ...search, page, page_size: pageSize })
            }
          />
        </CardContent>
      </Card>

      <Sheet
        open={selectedSessionId !== null}
        onOpenChange={(open) => {
          if (!open) {
            updateRouteSearch({ ...search, session_id: undefined })
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
                canAccessRequestLogs={session?.capabilities.platform_admin ?? false}
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
  // Filter params come from the chip registry, so every URL filter has a chip and vice versa.
  for (const param of sessionFilterParams) {
    if (param === 'minimum_coverage_percent') {
      const percent = integerInRange(search[param], 0, 100)
      if (percent !== undefined) result[param] = percent
      continue
    }
    const value = trimmedText(search[param])
    if (value) result[param] = value
  }
  const sessionId = trimmedText(search.session_id)
  if (sessionId) result.session_id = sessionId
  return result
}

function trimmedText(value: unknown) {
  return typeof value === 'string' ? value.trim() : ''
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
