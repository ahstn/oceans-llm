import { useTransition, type ReactNode } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'

import { PageHeader } from '@/components/layout/page-header'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import { getAttribution } from '@/server/admin-data.functions'
import type { AttributionReportView } from '@/types/api'

import { ChangesTable } from './-attribution/changes-table'
import { AttributionFilters, UserFilterButton } from './-attribution/controls'
import { AttributionFilteredEmpty, AttributionSetupEmpty } from './-attribution/empty-state'
import { AttributionKpis } from './-attribution/kpis'
import { ModelBreakdown } from './-attribution/model-breakdown'
import { PullRequestsTable } from './-attribution/pull-requests-table'
import {
  normalizeAttributionSearch,
  toAttributionWindow,
  type AttributionSearch,
} from './-attribution/shared'
import { UserActivityTable } from './-attribution/user-activity-table'

export const Route = createFileRoute('/observability/attribution')({
  validateSearch: (search: Record<string, unknown>) => normalizeAttributionSearch(search),
  loaderDeps: ({ search }) => search,
  loader: ({ deps }) => getAttribution({ data: deps }),
  component: AttributionPage,
})

// Git AI attribution joined with gateway traffic: KPIs, then users, models, PRs, and commits.
export function AttributionPage() {
  const { data: report } = Route.useLoaderData()
  const { session } = Route.useRouteContext()
  const search = Route.useSearch()
  const router = useRouter()
  const [isPending, startTransition] = useTransition()
  const isPlatformAdmin = isPlatformAdminSession(session)

  function updateSearch(next: Partial<AttributionSearch>) {
    startTransition(async () => {
      await router.navigate({
        to: '/observability/attribution',
        search: normalizeAttributionSearch({ ...search, ...next }),
      })
    })
  }

  // The report echoes the filters it applied, and the controls show those, so they always describe
  // the rendered report. For non-admins the gateway echoes their own id, which is not a filter.
  const filtered = Boolean((isPlatformAdmin && report.user_id) || report.repository)
  const selectedUser = report.users.find((row) => row.user.id === report.user_id)?.user

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Observability"
        title="Attribution"
        description={
          isPlatformAdmin
            ? 'See how much of each commit AI wrote, the pull requests it reached, and the agent requests and cost behind it, for every user.'
            : 'See how much of your commits AI wrote, the pull requests they reached, and the agent requests and cost behind them.'
        }
        actions={
          <div className="flex flex-wrap items-center gap-2">
            {isPlatformAdmin && report.user_id ? (
              <UserFilterButton
                name={selectedUser?.name ?? 'Selected user'}
                isPending={isPending}
                onClear={() => updateSearch({ user_id: undefined })}
              />
            ) : null}
            <AttributionFilters
              windowDays={toAttributionWindow(String(report.window_days)) ?? search.days}
              repository={report.repository ?? undefined}
              repositories={repositoryOptions(report)}
              isPending={isPending}
              onWindowChange={(days) => updateSearch({ days })}
              onRepositoryChange={(repository) => updateSearch({ repository })}
              onRefresh={() =>
                startTransition(async () => {
                  await router.invalidate()
                })
              }
            />
          </div>
        }
      />

      <TruncationAlert coverage={report.coverage} />

      <AttributionKpis report={report} pending={isPending} />

      <AttributionBody
        report={report}
        isPending={isPending}
        filtered={filtered}
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

/** Warns when a capped read left rows out of the totals. */
function TruncationAlert({ coverage }: { coverage: AttributionReportView['coverage'] }) {
  const truncated =
    coverage.commits_truncated ||
    coverage.pull_requests_truncated ||
    coverage.agent_sessions_truncated ||
    coverage.request_facts_truncated
  if (!truncated) return null
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

/** The report sections, or an empty state that tells filtered-out apart from not set up. */
function AttributionBody({
  report,
  isPending,
  filtered,
  onClearFilters,
  onSelectUser,
}: {
  report: AttributionReportView
  isPending: boolean
  filtered: boolean
  onClearFilters: () => void
  onSelectUser?: (userId: string) => void
}) {
  const empty =
    report.users.length === 0 && report.pull_requests.length === 0 && report.changes.length === 0
  if (!empty) {
    return <AttributionSections report={report} isPending={isPending} onSelectUser={onSelectUser} />
  }
  if (filtered) return <AttributionFilteredEmpty onClear={onClearFilters} />
  return <AttributionSetupEmpty windowDays={report.window_days} />
}

function AttributionSections({
  report,
  isPending,
  onSelectUser,
}: {
  report: AttributionReportView
  isPending: boolean
  /** Platform admins only: the report covers every user, so a row can narrow it to one. */
  onSelectUser?: (userId: string) => void
}) {
  return (
    <div
      className={
        isPending ? 'flex flex-col gap-6 opacity-60 transition-opacity' : 'flex flex-col gap-6'
      }
      aria-busy={isPending}
    >
      <ReportCard
        title="User activity"
        description={
          onSelectUser
            ? "Activity per user, matched by commit author email or else the key owner. Select a user's name to filter the report to them."
            : 'Activity per user, matched by commit author email or else the key owner.'
        }
      >
        <UserActivityTable
          users={report.users}
          selectedUserId={report.user_id}
          onSelectUser={onSelectUser}
        />
      </ReportCard>
      <ReportCard
        title="Models"
        description="Requests allocated to the window's commits and their priced cost, per gateway model, with each model's share of those requests."
      >
        <ModelBreakdown models={report.models} />
      </ReportCard>
      <ReportCard
        title="Merged pull requests"
        description={
          <>
            Pull requests reported by <code className="font-mono">oceans hook prs</code>, with the
            lines, requests, and cost of their original commits.
          </>
        }
      >
        <PullRequestsTable pullRequests={report.pull_requests} />
      </ReportCard>
      <ReportCard
        title="Recent changes"
        description="Each commit gets the requests its AI sessions made since their previous commit. Sessions with no matching gateway traffic are unjoined: their lines count, but they bring no requests or cost."
      >
        <ChangesTable changes={report.changes} pullRequests={report.pull_requests} />
      </ReportCard>
    </div>
  )
}

function ReportCard({
  title,
  description,
  children,
}: {
  title: string
  description: ReactNode
  children: ReactNode
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
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
