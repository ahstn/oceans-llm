import { useState, useTransition, type FormEvent } from 'react'
import { GitPullRequestIcon } from '@hugeicons/core-free-icons'
import { createFileRoute, useRouter, type SearchSchemaInput } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatCount, formatDuration, formatTimestamp } from '@/lib/format'
import { RepoDetailsDialog } from '@/routes/-review-agent/repo-details-dialog'
import {
  formatFeatureSummary,
  formatLineDelta,
  formatProvider,
  getErrorMessage,
  normalizeReviewAgentSearch,
  repoStatusVariant,
  runStatusVariant,
  runTimestamp,
  type RepoDetailsSection,
} from '@/routes/-review-agent/review-agent-meta'
import { ServiceAccountSelect } from '@/routes/-review-agent/service-account-select'
import { createReviewAgentRepo, getReviewAgentOverview } from '@/server/admin-data.functions'
import type {
  ReviewAgentOverviewPayload,
  ReviewAgentRepositoryView,
  ReviewAgentRunView,
  ServiceAccountView,
} from '@/types/api'

export const Route = createFileRoute('/review-agent')({
  validateSearch: (
    search: { repo_id?: string; repo_section?: RepoDetailsSection } & SearchSchemaInput,
  ) => normalizeReviewAgentSearch(search),
  loader: () => getReviewAgentOverview(),
  component: ReviewAgentPage,
})

interface CreateRepoForm {
  owner: string
  name: string
  service_account_id: string | null
}

const initialCreateForm: CreateRepoForm = {
  owner: '',
  name: '',
  service_account_id: null,
}

type OpenRepoDialog = (repository: ReviewAgentRepositoryView, section?: RepoDetailsSection) => void

/** Repositories, recent review runs, and the repository dialog selected by the URL. */
export function ReviewAgentPage() {
  const router = useRouter()
  const {
    data: { repositories, service_accounts: serviceAccounts, runs },
  } = Route.useLoaderData() as { data: ReviewAgentOverviewPayload }
  const search = Route.useSearch()

  const selectedRepo = search.repo_id
    ? (repositories.find((repository) => repository.id === search.repo_id) ?? null)
    : null

  function closeRepoDialog() {
    void router.navigate({ to: '/review-agent', search: {} })
  }

  const openRepoDialog: OpenRepoDialog = (repository, section = 'overview') => {
    void router.navigate({
      to: '/review-agent',
      search: { repo_id: repository.id, repo_section: section },
    })
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="Review agent"
        description="Set up automatic code reviews and review recent results."
      />

      <RepositoriesCard
        repositories={repositories}
        serviceAccounts={serviceAccounts}
        runs={runs}
        onManage={openRepoDialog}
      />

      <RecentReviewsCard repositories={repositories} runs={runs} />

      <RepoDetailsDialog
        repository={selectedRepo}
        section={search.repo_section}
        serviceAccounts={serviceAccounts}
        runs={runs}
        onSectionChange={(section) => {
          if (selectedRepo) {
            openRepoDialog(selectedRepo, section)
          }
        }}
        onClose={closeRepoDialog}
      />
    </div>
  )
}

function RepositoriesCard({
  repositories,
  serviceAccounts,
  runs,
  onManage,
}: {
  repositories: ReviewAgentRepositoryView[]
  serviceAccounts: ServiceAccountView[]
  runs: ReviewAgentRunView[]
  onManage: OpenRepoDialog
}) {
  const [isCreateOpen, setIsCreateOpen] = useState(false)

  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div className="flex flex-col gap-1">
          <CardTitle>Repositories</CardTitle>
          <CardDescription>
            Choose the repositories that use automatic code reviews.
          </CardDescription>
        </div>

        <AddRepositoryDialog
          open={isCreateOpen}
          onOpenChange={setIsCreateOpen}
          serviceAccounts={serviceAccounts}
          onCreated={(repository) => onManage(repository, 'setup')}
        />
      </CardHeader>

      <CardContent>
        {repositories.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <AppIcon icon={GitPullRequestIcon} size={22} stroke={1.5} />
              </EmptyMedia>
              <EmptyTitle>No repositories configured yet</EmptyTitle>
              <EmptyDescription>
                Configure the first repository, then copy the generated workflow into
                <code className="mx-1">.github/workflows/</code>
                to start reviewing pull requests.
              </EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              <Button type="button" onClick={() => setIsCreateOpen(true)}>
                Configure first repository
              </Button>
            </EmptyContent>
          </Empty>
        ) : (
          <RepositoryList
            repositories={repositories}
            serviceAccounts={serviceAccounts}
            runs={runs}
            onManage={onManage}
          />
        )}
      </CardContent>
    </Card>
  )
}

function AddRepositoryDialog({
  open,
  onOpenChange,
  serviceAccounts,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  serviceAccounts: ServiceAccountView[]
  onCreated: (repository: ReviewAgentRepositoryView) => void
}) {
  const router = useRouter()
  const [form, setForm] = useState<CreateRepoForm>(initialCreateForm)
  const [isPending, startTransition] = useTransition()

  function close() {
    setForm(initialCreateForm)
    onOpenChange(false)
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const serviceAccountId = form.service_account_id
    if (!serviceAccountId) {
      return
    }

    const owner = form.owner.trim()
    const name = form.name.trim()

    startTransition(async () => {
      try {
        const response = await createReviewAgentRepo({
          data: {
            provider: 'github',
            owner,
            name,
            full_name: `${owner}/${name}`,
            service_account_id: serviceAccountId,
          },
        })
        toast.success('Repository configured')
        close()
        await router.invalidate()
        onCreated(response.data.repository)
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next)
        if (!next) {
          setForm(initialCreateForm)
        }
      }}
    >
      <DialogTrigger asChild>
        <Button type="button">Add repository</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add repository</DialogTitle>
          <DialogDescription>
            Bind a GitHub repository to a service account, then copy the generated workflow into the
            repository to enable reviews.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-6" onSubmit={handleSubmit}>
          <AddRepositoryFields
            form={form}
            serviceAccounts={serviceAccounts}
            onFormChange={setForm}
          />

          <DialogFooter>
            <Button type="button" variant="secondary" onClick={close}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={isPending || serviceAccounts.length === 0 || !form.service_account_id}
            >
              {isPending ? 'Configuring…' : 'Configure repository'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function AddRepositoryFields({
  form,
  serviceAccounts,
  onFormChange,
}: {
  form: CreateRepoForm
  serviceAccounts: ServiceAccountView[]
  onFormChange: (form: CreateRepoForm) => void
}) {
  return (
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor="repo-owner">Owner</FieldLabel>
        <Input
          id="repo-owner"
          value={form.owner}
          onChange={(event) => onFormChange({ ...form, owner: event.target.value })}
          placeholder="ahstn"
          required
        />
      </Field>

      <Field>
        <FieldLabel htmlFor="repo-name">Repository name</FieldLabel>
        <Input
          id="repo-name"
          value={form.name}
          onChange={(event) => onFormChange({ ...form, name: event.target.value })}
          placeholder="oceans-llm"
          required
        />
        <FieldDescription>
          Reviews run for same-repository, non-draft pull requests only.
        </FieldDescription>
      </Field>

      {serviceAccounts.length === 0 ? (
        <Alert>
          <AlertTitle>No service accounts available</AlertTitle>
          <AlertDescription>
            The GitHub Action authenticates with a service account API key. Create a service account
            under Identity before configuring a repository.
          </AlertDescription>
        </Alert>
      ) : null}

      <Field>
        <FieldLabel htmlFor="repo-service-account">Service account</FieldLabel>
        <ServiceAccountSelect
          id="repo-service-account"
          value={form.service_account_id ?? undefined}
          serviceAccounts={serviceAccounts}
          onValueChange={(value) => onFormChange({ ...form, service_account_id: value })}
        />
        <FieldDescription>
          The action authenticates with an API key for this service account, and its model grants
          and budgets govern review model access.
        </FieldDescription>
      </Field>
    </FieldGroup>
  )
}

/** Repository cards on small screens, a table from `md` up. */
function RepositoryList({
  repositories,
  serviceAccounts,
  runs,
  onManage,
}: {
  repositories: ReviewAgentRepositoryView[]
  serviceAccounts: ServiceAccountView[]
  runs: ReviewAgentRunView[]
  onManage: OpenRepoDialog
}) {
  // Runs arrive most recent first, so the first run seen per repository is its last review.
  const lastRunByRepo = new Map<string, ReviewAgentRunView>()
  for (const run of runs) {
    if (!lastRunByRepo.has(run.repository_id)) {
      lastRunByRepo.set(run.repository_id, run)
    }
  }
  const serviceAccountById = new Map(serviceAccounts.map((account) => [account.id, account]))

  return (
    <div className="flex flex-col gap-4">
      <RepositoryCards
        repositories={repositories}
        lastRunByRepo={lastRunByRepo}
        onManage={onManage}
      />
      <RepositoryTable
        repositories={repositories}
        lastRunByRepo={lastRunByRepo}
        serviceAccountById={serviceAccountById}
        onManage={onManage}
      />
    </div>
  )
}

function RepositoryCards({
  repositories,
  lastRunByRepo,
  onManage,
}: {
  repositories: ReviewAgentRepositoryView[]
  lastRunByRepo: Map<string, ReviewAgentRunView>
  onManage: OpenRepoDialog
}) {
  return (
    <div className="grid gap-3 md:hidden">
      {repositories.map((repository) => {
        const lastRun = lastRunByRepo.get(repository.id)

        return (
          <article
            key={repository.id}
            className="border-border bg-surface-muted rounded-lg border p-4"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex min-w-0 items-center gap-3">
                <GeneratedAvatar kind="team" name={repository.full_name} size={40} />
                <div className="min-w-0">
                  <p className="text-foreground truncate font-semibold">{repository.full_name}</p>
                  <p className="text-subtle-foreground truncate text-sm">
                    {formatProvider(repository.provider)}
                  </p>
                </div>
              </div>
              <Badge variant={repoStatusVariant(repository.status)}>{repository.status}</Badge>
            </div>

            <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
              <div>
                <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
                  Features
                </dt>
                <dd className="text-subtle-foreground">
                  {formatFeatureSummary(repository.settings)}
                </dd>
              </div>
              <div>
                <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
                  Last review
                </dt>
                <dd className="text-subtle-foreground">
                  {lastRun ? formatTimestamp(runTimestamp(lastRun)) : 'Never'}
                </dd>
              </div>
            </dl>

            <div className="mt-4 flex flex-wrap gap-2">
              <Button
                type="button"
                size="sm"
                variant="secondary"
                onClick={() => onManage(repository)}
              >
                Manage
              </Button>
            </div>
          </article>
        )
      })}
    </div>
  )
}

function RepositoryTable({
  repositories,
  lastRunByRepo,
  serviceAccountById,
  onManage,
}: {
  repositories: ReviewAgentRepositoryView[]
  lastRunByRepo: Map<string, ReviewAgentRunView>
  serviceAccountById: Map<string, ServiceAccountView>
  onManage: OpenRepoDialog
}) {
  return (
    <div className="border-border hidden overflow-hidden rounded-md border md:block">
      <Table className="text-left">
        <AdminTableHeader
          columns={[
            'Repository',
            'Service account',
            'Features',
            'Last review',
            'Status',
            'Actions',
          ]}
        />
        <TableBody>
          {repositories.map((repository) => {
            const lastRun = lastRunByRepo.get(repository.id)
            const serviceAccount = serviceAccountById.get(repository.service_account_id)

            return (
              <TableRow key={repository.id}>
                <TableCell className="text-foreground px-3 py-3">
                  <div className="flex min-w-0 items-center gap-3">
                    <GeneratedAvatar kind="team" name={repository.full_name} size={32} />
                    <div className="min-w-0">
                      <p className="truncate">{repository.full_name}</p>
                      <p className="text-subtle-foreground truncate text-xs">
                        {formatProvider(repository.provider)}
                      </p>
                    </div>
                  </div>
                </TableCell>
                <TableCell className="text-subtle-foreground px-3 py-3">
                  {serviceAccount?.name ?? '—'}
                </TableCell>
                <TableCell className="text-subtle-foreground px-3 py-3">
                  {formatFeatureSummary(repository.settings)}
                </TableCell>
                <TableCell className="text-subtle-foreground px-3 py-3">
                  {lastRun ? (
                    <div className="flex items-center gap-2">
                      <Badge variant={runStatusVariant(lastRun.status)}>{lastRun.status}</Badge>
                      <span className="text-xs">{formatTimestamp(runTimestamp(lastRun))}</span>
                    </div>
                  ) : (
                    'Never'
                  )}
                </TableCell>
                <TableCell className="px-3 py-3">
                  <Badge variant={repoStatusVariant(repository.status)}>{repository.status}</Badge>
                </TableCell>
                <TableCell className="px-3 py-3">
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    onClick={() => onManage(repository)}
                  >
                    Manage
                  </Button>
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

function RecentReviewsCard({
  repositories,
  runs,
}: {
  repositories: ReviewAgentRepositoryView[]
  runs: ReviewAgentRunView[]
}) {
  const [repoFilter, setRepoFilter] = useState<string>('all')
  const visibleRuns =
    repoFilter === 'all' ? runs : runs.filter((run) => run.repository_id === repoFilter)
  const repositoryById = new Map(repositories.map((repository) => [repository.id, repository]))

  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div className="flex flex-col gap-1">
          <CardTitle>Recent reviews</CardTitle>
          <CardDescription>
            Review runs reported by the GitHub Action, most recent first.
          </CardDescription>
        </div>

        {repositories.length > 0 ? (
          <Select value={repoFilter} onValueChange={setRepoFilter}>
            <SelectTrigger className="w-56" aria-label="Filter runs by repository">
              <SelectValue placeholder="All repositories" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All repositories</SelectItem>
                {repositories.map((repository) => (
                  <SelectItem key={repository.id} value={repository.id}>
                    {repository.full_name}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        ) : null}
      </CardHeader>

      <CardContent>
        {visibleRuns.length === 0 ? (
          <p className="text-subtle-foreground text-sm">
            No review runs recorded yet. Runs appear here after the workflow executes on a pull
            request.
          </p>
        ) : (
          <div className="border-border overflow-hidden rounded-md border">
            <Table className="text-left">
              <AdminTableHeader
                columns={[
                  'Repository',
                  'Status',
                  'Commit',
                  'Model',
                  { label: 'Files', align: 'right' },
                  { label: 'Lines', align: 'right' },
                  { label: 'Comments', align: 'right' },
                  { label: 'Duration', align: 'right' },
                  'When',
                ]}
              />
              <TableBody>
                {visibleRuns.map((run) => (
                  <TableRow key={run.id}>
                    <TableCell className="text-foreground px-3 py-3">
                      {repositoryById.get(run.repository_id)?.full_name ?? '—'}
                    </TableCell>
                    <TableCell className="px-3 py-3">
                      <Badge variant={runStatusVariant(run.status)}>{run.status}</Badge>
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3 font-mono text-xs">
                      {run.head_sha ? run.head_sha.slice(0, 7) : '—'}
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3">
                      {run.model_key ?? '—'}
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3 text-right tabular-nums">
                      {formatCount(run.files_changed)}
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3 text-right tabular-nums">
                      {formatLineDelta(run)}
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3 text-right tabular-nums">
                      {formatCount(run.inline_comments_created)}
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3 text-right tabular-nums">
                      {formatDuration(run.duration_ms)}
                    </TableCell>
                    <TableCell className="text-subtle-foreground px-3 py-3">
                      {formatTimestamp(runTimestamp(run))}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
