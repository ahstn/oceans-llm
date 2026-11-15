import { lazy, Suspense, useTransition, type ComponentType } from 'react'
import { createFileRoute, Link, useRouter } from '@tanstack/react-router'

import { PageHeader } from '@/components/layout/page-header'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import { getAttribution } from '@/server/admin-data.functions'
import type { AttributionReportView } from '@/types/api'

import { AttributionFilters, UserFilterButton } from './-attribution/controls'
import { AttributionFilteredEmpty, AttributionSetupEmpty } from './-attribution/empty-state'
import {
  attributionReportInput,
  enabledAttributionTabs,
  resolveAttributionTab,
  TAB_LABELS,
  TAB_QUESTIONS,
  type AttributionTab,
} from './-attribution/insights'
import {
  normalizeAttributionSearch,
  toAttributionWindow,
  type AttributionSearch,
} from './-attribution/shared'

type TabProps = {
  report: AttributionReportView
  onSelectUser?: (userId: string) => void
  name: string
}

// Each tab loads on its own, so Recharts stays out of the route chunk.
const TABS: Record<AttributionTab, ComponentType<TabProps>> = {
  impact: lazy(() => import('./-attribution/tabs/impact-tab')),
  delivery: lazy(() => import('./-attribution/tabs/delivery-tab')),
  pull_requests: lazy(() => import('./-attribution/tabs/pull-requests-tab')),
  repositories: lazy(() => import('./-attribution/tabs/repositories-tab')),
  durability: lazy(() => import('./-attribution/tabs/durability-tab')),
  my_work: lazy(() => import('./-attribution/tabs/my-work-tab')),
}

export const Route = createFileRoute('/observability/attribution')({
  validateSearch: (search: Record<string, unknown>) => normalizeAttributionSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps, context }) =>
    getAttribution({ data: attributionReportInput(deps, context.session) }),
  component: AttributionPage,
})

// Git AI attribution joined with gateway traffic, as tabs the gateway config turns on.
export function AttributionPage() {
  const { data: report } = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const search = Route.useSearch()
  const router = useRouter()
  const [isPending, startTransition] = useTransition()
  const isPlatformAdmin = isPlatformAdminSession(session)
  const tabs = enabledAttributionTabs(session)
  const tab = resolveAttributionTab(search.tab, session)

  function updateSearch(next: Partial<AttributionSearch>) {
    startTransition(async () => {
      await router.navigate({
        to: '/observability/attribution',
        search: normalizeAttributionSearch({ ...search, ...next }),
      })
    })
  }

  if (!tab) {
    return (
      <div className="flex min-w-0 flex-1 flex-col gap-6">
        <PageHeader
          section="Observability"
          title="Attribution"
          description="Git AI attribution joined with gateway traffic."
        />
        <TabsOffEmpty
          conceptsEnabled={session?.capabilities.attribution_tabs.includes('concepts') ?? false}
        />
      </div>
    )
  }

  // The report echoes the filters it applied, and the controls show those, so they always describe
  // the rendered report. For non-admins, and on My work, the gateway echoes their own id, which is
  // not a filter.
  const userFiltered = isPlatformAdmin && tab !== 'my_work' && Boolean(report.user_id)

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Attribution"
        description={TAB_QUESTIONS[tab]}
        actions={
          <AttributionActions
            report={report}
            fallbackDays={search.days}
            userFiltered={userFiltered}
            isPending={isPending}
            onUpdate={updateSearch}
            onRefresh={() =>
              startTransition(async () => {
                await router.invalidate()
              })
            }
          />
        }
      />
      <AttributionNav
        tabs={tabs}
        tab={tab}
        search={search}
        conceptsEnabled={session?.capabilities.attribution_tabs.includes('concepts') ?? false}
      />
      <TruncationAlert report={report} />
      <AttributionBody
        tab={tab}
        report={report}
        isPending={isPending}
        filtered={userFiltered || Boolean(report.repository)}
        name={session?.user.name ?? 'You'}
        onClearFilters={() => updateSearch({ user_id: undefined, repository: undefined })}
        onSelectUser={
          isPlatformAdmin
            ? (userId) => updateSearch({ user_id: userId === report.user_id ? undefined : userId })
            : undefined
        }
      />
    </div>
  )
}

/** The user filter (when one applies), window, repository and refresh controls. */
function AttributionActions({
  report,
  fallbackDays,
  userFiltered,
  isPending,
  onUpdate,
  onRefresh,
}: {
  report: AttributionReportView
  fallbackDays: AttributionSearch['days']
  userFiltered: boolean
  isPending: boolean
  onUpdate: (next: Partial<AttributionSearch>) => void
  onRefresh: () => void
}) {
  const selectedUser = report.users.find((row) => row.user.id === report.user_id)?.user
  return (
    <div className="flex flex-wrap items-center gap-2">
      {userFiltered ? (
        <UserFilterButton
          name={selectedUser?.name ?? 'Selected user'}
          isPending={isPending}
          onClear={() => onUpdate({ user_id: undefined })}
        />
      ) : null}
      <AttributionFilters
        windowDays={toAttributionWindow(String(report.window_days)) ?? fallbackDays}
        repository={report.repository ?? undefined}
        repositories={repositoryOptions(report)}
        isPending={isPending}
        onWindowChange={(days) => onUpdate({ days })}
        onRepositoryChange={(repository) => onUpdate({ repository })}
        onRefresh={onRefresh}
      />
    </div>
  )
}

/** The enabled tabs, like MCP's section nav, plus the concepts link when its flag is on. */
function AttributionNav({
  tabs,
  tab,
  search,
  conceptsEnabled,
}: {
  tabs: AttributionTab[]
  tab: AttributionTab
  search: AttributionSearch
  conceptsEnabled: boolean
}) {
  return (
    <nav
      aria-label="Attribution sections"
      className="flex flex-wrap items-center gap-2 border-b pb-4"
    >
      {tabs.map((entry) => (
        <Button key={entry} asChild variant={entry === tab ? 'secondary' : 'ghost'}>
          <Link
            to="/observability/attribution"
            search={{ ...search, tab: entry }}
            aria-current={entry === tab ? 'page' : undefined}
          >
            {TAB_LABELS[entry]}
          </Link>
        </Button>
      ))}
      {conceptsEnabled ? (
        <Button asChild variant="link" className="ml-auto">
          <Link to="/observability/attribution/concepts" search={{ tab: 'impact', days: 30 }}>
            Concepts
          </Link>
        </Button>
      ) : null}
    </nav>
  )
}

function TabsOffEmpty({ conceptsEnabled }: { conceptsEnabled: boolean }) {
  return (
    <Empty className="bg-surface-muted rounded-xl border">
      <EmptyHeader>
        <EmptyTitle>
          {conceptsEnabled
            ? 'Every report tab is turned off'
            : 'Every Attribution tab is turned off'}
        </EmptyTitle>
        <EmptyDescription>
          Turn tabs on under <code className="font-mono">attribution.pages</code> in the gateway
          config.{conceptsEnabled ? ' The concept mockups are still on.' : ''}
        </EmptyDescription>
      </EmptyHeader>
      {conceptsEnabled ? (
        <EmptyContent>
          <Button asChild variant="outline">
            <Link to="/observability/attribution/concepts" search={{ tab: 'impact', days: 30 }}>
              Open the concepts
            </Link>
          </Button>
        </EmptyContent>
      ) : null}
    </Empty>
  )
}

/** Warns when a capped read left rows out of the totals, or out of the previous window. */
function TruncationAlert({ report }: { report: AttributionReportView }) {
  const { coverage } = report
  const truncated =
    coverage.commits_truncated ||
    coverage.pull_requests_truncated ||
    coverage.agent_sessions_truncated ||
    coverage.request_facts_truncated
  if (truncated) {
    return (
      <Alert>
        <AlertTitle>This report is incomplete</AlertTitle>
        <AlertDescription>
          The window holds more commits, pull requests, agent sessions, or requests than one report
          reads, so some are left out of these totals. Choose a shorter window, or filter by user or
          repository.
        </AlertDescription>
      </Alert>
    )
  }
  if (!report.previous_truncated) return null
  return (
    <Alert>
      <AlertTitle>No comparison with the previous window</AlertTitle>
      <AlertDescription>
        The previous window holds more rows than one report reads, so changes against it would be
        wrong and are hidden. Choose a shorter window, or filter by user or repository.
      </AlertDescription>
    </Alert>
  )
}

/** The tab, or an empty state that tells filtered-out apart from not set up. */
function AttributionBody({
  tab,
  report,
  isPending,
  filtered,
  name,
  onClearFilters,
  onSelectUser,
}: {
  tab: AttributionTab
  report: AttributionReportView
  isPending: boolean
  filtered: boolean
  name: string
  onClearFilters: () => void
  onSelectUser?: (userId: string) => void
}) {
  const empty =
    report.users.length === 0 &&
    report.pull_requests.length === 0 &&
    report.changes.length === 0 &&
    report.session_outcomes.length === 0
  if (empty) {
    if (filtered) return <AttributionFilteredEmpty onClear={onClearFilters} />
    return <AttributionSetupEmpty windowDays={report.window_days} />
  }
  const Tab = TABS[tab]
  return (
    <div
      className={isPending ? 'min-w-0 opacity-60 transition-opacity' : 'min-w-0'}
      aria-busy={isPending}
    >
      <Suspense fallback={<TabSkeleton />}>
        <Tab report={report} onSelectUser={onSelectUser} name={name} />
      </Suspense>
    </div>
  )
}

function TabSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy>
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {['a', 'b', 'c', 'd'].map((key) => (
          <Skeleton key={key} className="h-32 rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-80 rounded-xl" />
    </div>
  )
}

/** Repositories in the report, plus the active filter so it stays selectable. */
function repositoryOptions(report: AttributionReportView) {
  const names = new Set<string>()
  if (report.repository) names.add(report.repository)
  for (const change of report.changes) names.add(change.repository)
  for (const pullRequest of report.pull_requests) names.add(pullRequest.repository)
  return [...names].sort()
}
