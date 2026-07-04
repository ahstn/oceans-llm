import { useEffect, useState, useTransition, type CSSProperties, type FormEvent } from 'react'
import {
  Cancel01Icon,
  Configuration01Icon,
  GitPullRequestIcon,
  SourceCodeIcon,
  WorkflowSquare01Icon,
} from '@hugeicons/core-free-icons'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogClose,
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
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
} from '@/components/ui/sidebar'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  createReviewAgentRepo,
  disableReviewAgentRepo,
  getReviewAgentOverview,
  reactivateReviewAgentRepo,
  renderReviewAgentRepoWorkflow,
  updateReviewAgentRepo,
} from '@/server/admin-data.functions'
import type {
  ReviewAgentOverviewPayload,
  ReviewAgentRepositoryView,
  ReviewAgentRunView,
  ReviewAgentSettingsView,
  ReviewAgentWorkflowPayload,
  ServiceAccountView,
} from '@/types/api'

export const Route = createFileRoute('/review-agent')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  validateSearch: (search: Record<string, unknown>) => normalizeReviewAgentSearch(search),
  loader: () => getReviewAgentOverview(),
  component: ReviewAgentPage,
})

const defaultSettings: ReviewAgentSettingsView = {
  inline_review_enabled: true,
  pr_summary_enabled: true,
  diagrams_enabled: false,
  linked_issue_detection_enabled: true,
  linked_issue_assessment_enabled: false,
  max_inline_comments: null,
  default_model_key: null,
  request_changes_on_high_severity: false,
}

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

interface RepoUpdateForm {
  service_account_id: string
  settings: ReviewAgentSettingsView
}

interface WorkflowForm {
  action_ref: string
  api_key_secret_name: string
}

const initialWorkflowForm: WorkflowForm = {
  action_ref: '',
  api_key_secret_name: '',
}

const repoDetailsSections = [
  { id: 'overview', label: 'Overview', icon: SourceCodeIcon },
  { id: 'settings', label: 'Review settings', icon: Configuration01Icon },
  { id: 'setup', label: 'Workflow setup', icon: WorkflowSquare01Icon },
] as const

type RepoDetailsSection = (typeof repoDetailsSections)[number]['id']

const featureToggles = [
  {
    key: 'inline_review_enabled',
    label: 'Inline review',
    description: 'Post inline findings on lines changed by the pull request.',
  },
  {
    key: 'pr_summary_enabled',
    label: 'PR summary',
    description: 'Include a summary of the changes in the managed top-level comment.',
  },
  {
    key: 'diagrams_enabled',
    label: 'Sequence diagrams',
    description: 'Include a diagram section in the managed comment when available.',
  },
  {
    key: 'linked_issue_detection_enabled',
    label: 'Linked issue detection',
    description: 'Detect issues referenced by the pull request and record the links.',
  },
  {
    key: 'linked_issue_assessment_enabled',
    label: 'Linked issue assessment',
    description: 'Assess whether the pull request addresses its linked issues.',
  },
] as const

export function ReviewAgentPage() {
  const router = useRouter()
  const {
    data: { repositories, service_accounts: serviceAccounts, runs },
  } = Route.useLoaderData() as { data: ReviewAgentOverviewPayload }
  const search = Route.useSearch()
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [createForm, setCreateForm] = useState<CreateRepoForm>(initialCreateForm)
  const [updateForm, setUpdateForm] = useState<RepoUpdateForm | null>(null)
  const [runsRepoFilter, setRunsRepoFilter] = useState<string>('all')
  const [workflowForm, setWorkflowForm] = useState<WorkflowForm>(initialWorkflowForm)
  const [workflow, setWorkflow] = useState<ReviewAgentWorkflowPayload | null>(null)
  const [isWorkflowPending, setIsWorkflowPending] = useState(false)
  const [workflowError, setWorkflowError] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()

  const selectedRepo = search.repo_id
    ? (repositories.find((repository) => repository.id === search.repo_id) ?? null)
    : null
  const selectedRepoSection = search.repo_section

  const lastRunByRepo = new Map<string, ReviewAgentRunView>()
  for (const run of runs) {
    if (!lastRunByRepo.has(run.repository_id)) {
      lastRunByRepo.set(run.repository_id, run)
    }
  }

  const visibleRuns =
    runsRepoFilter === 'all' ? runs : runs.filter((run) => run.repository_id === runsRepoFilter)

  useEffect(() => {
    if (!selectedRepo) {
      setUpdateForm(null)
      setWorkflow(null)
      setWorkflowError(null)
      setWorkflowForm(initialWorkflowForm)
      return
    }

    setUpdateForm({
      service_account_id: selectedRepo.service_account_id,
      settings: { ...defaultSettings, ...selectedRepo.settings },
    })
    setWorkflow(null)
    setWorkflowError(null)
    setWorkflowForm(initialWorkflowForm)
  }, [selectedRepo])

  useEffect(() => {
    if (
      selectedRepo &&
      selectedRepoSection === 'setup' &&
      !workflow &&
      !isWorkflowPending &&
      !workflowError
    ) {
      void generateWorkflow()
    }
  }, [selectedRepo, selectedRepoSection, workflow, isWorkflowPending, workflowError])

  function resetCreateDialog() {
    setCreateForm(initialCreateForm)
    setIsCreateOpen(false)
  }

  function closeRepoDialog() {
    void router.navigate({ to: '/review-agent', search: {} })
  }

  function openRepoDialog(
    repository: ReviewAgentRepositoryView,
    section: RepoDetailsSection = 'overview',
  ) {
    void router.navigate({
      to: '/review-agent',
      search: { repo_id: repository.id, repo_section: section },
    })
  }

  function setSelectedRepoSection(section: RepoDetailsSection) {
    if (!selectedRepo) {
      return
    }

    void router.navigate({
      to: '/review-agent',
      search: { repo_id: selectedRepo.id, repo_section: section },
    })
  }

  async function refreshOverview() {
    await router.invalidate()
  }

  async function handleCreateRepo(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!createForm.service_account_id) {
      return
    }

    const owner = createForm.owner.trim()
    const name = createForm.name.trim()

    startTransition(async () => {
      try {
        const response = await createReviewAgentRepo({
          data: {
            provider: 'github',
            owner,
            name,
            full_name: `${owner}/${name}`,
            service_account_id: createForm.service_account_id!,
          },
        })
        toast.success('Repository configured')
        resetCreateDialog()
        await refreshOverview()
        openRepoDialog(response.data.repository, 'setup')
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleUpdateRepo(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedRepo || !updateForm) {
      return
    }

    startTransition(async () => {
      try {
        await updateReviewAgentRepo({
          data: {
            repositoryId: selectedRepo.id,
            input: {
              external_repository_id: selectedRepo.external_repository_id ?? null,
              full_name: selectedRepo.full_name,
              name: selectedRepo.name,
              owner: selectedRepo.owner,
              service_account_id: updateForm.service_account_id,
              settings: sanitizeSettings(updateForm.settings),
              settings_json: selectedRepo.settings_json,
              status: selectedRepo.status,
            },
          },
        })
        toast.success('Repository updated')
        await refreshOverview()
        closeRepoDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleDisableRepo() {
    if (!selectedRepo) {
      return
    }

    startTransition(async () => {
      try {
        await disableReviewAgentRepo({ data: { repositoryId: selectedRepo.id } })
        toast.success('Repository disabled')
        await refreshOverview()
        closeRepoDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleReactivateRepo() {
    if (!selectedRepo) {
      return
    }

    startTransition(async () => {
      try {
        await reactivateReviewAgentRepo({ data: { repositoryId: selectedRepo.id } })
        toast.success('Repository reactivated')
        await refreshOverview()
        closeRepoDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function generateWorkflow() {
    if (!selectedRepo) {
      return
    }

    setIsWorkflowPending(true)
    setWorkflowError(null)
    try {
      const response = await renderReviewAgentRepoWorkflow({
        data: {
          repositoryId: selectedRepo.id,
          input: {
            action_ref: workflowForm.action_ref.trim() || null,
            api_key_secret_name: workflowForm.api_key_secret_name.trim() || null,
          },
        },
      })
      setWorkflow(response.data)
    } catch (error) {
      setWorkflowError(getErrorMessage(error))
    } finally {
      setIsWorkflowPending(false)
    }
  }

  async function handleCopy(value: string, message: string) {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(message)
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  const serviceAccountById = new Map(serviceAccounts.map((account) => [account.id, account]))

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Review Agent</CardTitle>
            <CardDescription>
              Configure repositories for Pi-powered pull request reviews, then copy the generated
              GitHub Actions workflow into each repository to finish onboarding.
            </CardDescription>
          </div>

          <Dialog
            open={isCreateOpen}
            onOpenChange={(open) => {
              setIsCreateOpen(open)
              if (!open) {
                setCreateForm(initialCreateForm)
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
                  Bind a GitHub repository to a service account, then copy the generated workflow
                  into the repository to enable reviews.
                </DialogDescription>
              </DialogHeader>

              <form className="flex flex-col gap-6" onSubmit={handleCreateRepo}>
                <FieldGroup>
                  <Field>
                    <FieldLabel htmlFor="repo-owner">Owner</FieldLabel>
                    <Input
                      id="repo-owner"
                      value={createForm.owner}
                      onChange={(event) =>
                        setCreateForm((current) => ({ ...current, owner: event.target.value }))
                      }
                      placeholder="ahstn"
                      required
                    />
                  </Field>

                  <Field>
                    <FieldLabel htmlFor="repo-name">Repository name</FieldLabel>
                    <Input
                      id="repo-name"
                      value={createForm.name}
                      onChange={(event) =>
                        setCreateForm((current) => ({ ...current, name: event.target.value }))
                      }
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
                        The GitHub Action authenticates with a service account API key. Create a
                        service account under Identity before configuring a repository.
                      </AlertDescription>
                    </Alert>
                  ) : null}

                  <Field>
                    <FieldLabel htmlFor="repo-service-account">Service account</FieldLabel>
                    <Select
                      value={createForm.service_account_id ?? undefined}
                      onValueChange={(value) =>
                        setCreateForm((current) => ({ ...current, service_account_id: value }))
                      }
                    >
                      <SelectTrigger id="repo-service-account">
                        <SelectValue placeholder="Select service account" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {serviceAccounts.map((account) => (
                            <SelectItem key={account.id} value={account.id}>
                              {formatServiceAccount(account)}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      The action authenticates with an API key for this service account, and its
                      model grants and budgets govern review model access.
                    </FieldDescription>
                  </Field>
                </FieldGroup>

                <DialogFooter>
                  <Button type="button" variant="secondary" onClick={resetCreateDialog}>
                    Cancel
                  </Button>
                  <Button
                    type="submit"
                    disabled={
                      isPending || serviceAccounts.length === 0 || !createForm.service_account_id
                    }
                  >
                    {isPending ? 'Configuring…' : 'Configure repository'}
                  </Button>
                </DialogFooter>
              </form>
            </DialogContent>
          </Dialog>
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
            <div className="flex flex-col gap-4">
              <div className="grid gap-3 md:hidden">
                {repositories.map((repository) => {
                  const lastRun = lastRunByRepo.get(repository.id)

                  return (
                    <article
                      key={repository.id}
                      className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="flex min-w-0 items-center gap-3">
                          <GeneratedAvatar kind="team" name={repository.full_name} size={40} />
                          <div className="min-w-0">
                            <p className="truncate font-semibold text-[var(--color-text)]">
                              {repository.full_name}
                            </p>
                            <p className="truncate text-sm text-[var(--color-text-muted)]">
                              {formatProvider(repository.provider)}
                            </p>
                          </div>
                        </div>
                        <Badge variant={repoStatusVariant(repository.status)}>
                          {repository.status}
                        </Badge>
                      </div>

                      <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
                        <div>
                          <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                            Features
                          </dt>
                          <dd className="text-[var(--color-text-muted)]">
                            {formatFeatureSummary(repository.settings)}
                          </dd>
                        </div>
                        <div>
                          <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
                            Last review
                          </dt>
                          <dd className="text-[var(--color-text-muted)]">
                            {lastRun ? formatTimestamp(runTimestamp(lastRun)) : 'Never'}
                          </dd>
                        </div>
                      </dl>

                      <div className="mt-4 flex flex-wrap gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          onClick={() => openRepoDialog(repository)}
                        >
                          Manage
                        </Button>
                      </div>
                    </article>
                  )
                })}
              </div>

              <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
                <Table className="text-left">
                  <TableHeader className="bg-[color:var(--color-surface-muted)]">
                    <TableRow>
                      <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Repository
                      </TableHead>
                      <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Service account
                      </TableHead>
                      <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Features
                      </TableHead>
                      <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Last review
                      </TableHead>
                      <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Status
                      </TableHead>
                      <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                        Actions
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {repositories.map((repository) => {
                      const lastRun = lastRunByRepo.get(repository.id)
                      const serviceAccount = serviceAccountById.get(repository.service_account_id)

                      return (
                        <TableRow key={repository.id}>
                          <TableCell className="px-3 py-3 text-[var(--color-text)]">
                            <div className="flex min-w-0 items-center gap-3">
                              <GeneratedAvatar kind="team" name={repository.full_name} size={32} />
                              <div className="min-w-0">
                                <p className="truncate">{repository.full_name}</p>
                                <p className="truncate text-xs text-[var(--color-text-muted)]">
                                  {formatProvider(repository.provider)}
                                </p>
                              </div>
                            </div>
                          </TableCell>
                          <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                            {serviceAccount?.name ?? '—'}
                          </TableCell>
                          <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                            {formatFeatureSummary(repository.settings)}
                          </TableCell>
                          <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                            {lastRun ? (
                              <div className="flex items-center gap-2">
                                <Badge variant={runStatusVariant(lastRun.status)}>
                                  {lastRun.status}
                                </Badge>
                                <span className="text-xs">
                                  {formatTimestamp(runTimestamp(lastRun))}
                                </span>
                              </div>
                            ) : (
                              'Never'
                            )}
                          </TableCell>
                          <TableCell className="px-3 py-3">
                            <Badge variant={repoStatusVariant(repository.status)}>
                              {repository.status}
                            </Badge>
                          </TableCell>
                          <TableCell className="px-3 py-3">
                            <Button
                              type="button"
                              size="sm"
                              variant="secondary"
                              onClick={() => openRepoDialog(repository)}
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
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>Recent reviews</CardTitle>
            <CardDescription>
              Review runs reported by the GitHub Action, most recent first.
            </CardDescription>
          </div>

          {repositories.length > 0 ? (
            <Select value={runsRepoFilter} onValueChange={setRunsRepoFilter}>
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
            <p className="text-sm text-[var(--color-text-muted)]">
              No review runs recorded yet. Runs appear here after the workflow executes on a pull
              request.
            </p>
          ) : (
            <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
              <Table className="text-left">
                <TableHeader className="bg-[color:var(--color-surface-muted)]">
                  <TableRow>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Repository
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Status
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Commit
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Model
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Files
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Lines
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Comments
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      Duration
                    </TableHead>
                    <TableHead className="px-3 py-2 font-semibold text-[var(--color-text-soft)]">
                      When
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {visibleRuns.map((run) => {
                    const repository = repositories.find(
                      (candidate) => candidate.id === run.repository_id,
                    )

                    return (
                      <TableRow key={run.id}>
                        <TableCell className="px-3 py-3 text-[var(--color-text)]">
                          {repository?.full_name ?? '—'}
                        </TableCell>
                        <TableCell className="px-3 py-3">
                          <Badge variant={runStatusVariant(run.status)}>{run.status}</Badge>
                        </TableCell>
                        <TableCell className="px-3 py-3 font-mono text-xs text-[var(--color-text-muted)]">
                          {run.head_sha ? run.head_sha.slice(0, 7) : '—'}
                        </TableCell>
                        <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                          {run.model_key ?? '—'}
                        </TableCell>
                        <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                          {run.files_changed ?? '—'}
                        </TableCell>
                        <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                          {formatLineDelta(run)}
                        </TableCell>
                        <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                          {run.inline_comments_created ?? '—'}
                        </TableCell>
                        <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                          {formatDuration(run.duration_ms)}
                        </TableCell>
                        <TableCell className="px-3 py-3 text-[var(--color-text-muted)]">
                          {formatTimestamp(runTimestamp(run))}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>

      <Dialog
        open={Boolean(selectedRepo)}
        onOpenChange={(open) => {
          if (!open) {
            closeRepoDialog()
          }
        }}
      >
        <DialogContent
          showCloseButton={false}
          className="overflow-hidden p-0 md:max-h-[680px] md:max-w-[920px]"
        >
          <DialogTitle className="sr-only">Manage repository</DialogTitle>
          <DialogDescription className="sr-only">
            Review repository status, review settings, and workflow setup.
          </DialogDescription>

          {selectedRepo && updateForm ? (
            <SidebarProvider
              className="min-h-0 items-start"
              style={{ '--sidebar-width': '14rem' } as CSSProperties}
            >
              <Sidebar
                collapsible="none"
                className="hidden border-r border-[color:var(--color-border)] md:flex"
              >
                <SidebarContent className="p-3">
                  <SidebarGroup className="px-0 py-0">
                    <SidebarGroupContent>
                      <SidebarMenu className="gap-1">
                        {repoDetailsSections.map((section) => (
                          <SidebarMenuItem key={section.id}>
                            <SidebarMenuButton
                              type="button"
                              className="h-10 px-3 py-2"
                              isActive={selectedRepoSection === section.id}
                              onClick={() => setSelectedRepoSection(section.id)}
                            >
                              <AppIcon icon={section.icon} stroke={1.5} aria-hidden />
                              <span>{section.label}</span>
                            </SidebarMenuButton>
                          </SidebarMenuItem>
                        ))}
                      </SidebarMenu>
                    </SidebarGroupContent>
                  </SidebarGroup>
                </SidebarContent>
              </Sidebar>

              <main className="flex max-h-[680px] min-h-[520px] flex-1 flex-col overflow-hidden">
                <header className="flex shrink-0 flex-col gap-4 border-b border-[color:var(--color-border)] px-6 py-5">
                  <div className="flex items-start gap-3">
                    <GeneratedAvatar kind="team" name={selectedRepo.full_name} size={44} />
                    <div className="min-w-0 flex-1 pt-0.5">
                      <h2 className="truncate text-lg leading-tight font-semibold text-[var(--color-text)]">
                        {selectedRepo.full_name}
                      </h2>
                      <p className="mt-1 truncate text-sm text-[var(--color-text-muted)]">
                        {formatProvider(selectedRepo.provider)} ·{' '}
                        {serviceAccountById.get(selectedRepo.service_account_id)?.name ??
                          'Unknown service account'}
                      </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                      <Badge variant={repoStatusVariant(selectedRepo.status)}>
                        {selectedRepo.status}
                      </Badge>
                      <DialogClose asChild>
                        <Button type="button" variant="ghost" size="icon-sm" aria-label="Close">
                          <AppIcon icon={Cancel01Icon} stroke={1.5} aria-hidden />
                        </Button>
                      </DialogClose>
                    </div>
                  </div>

                  <div className="flex gap-2 overflow-x-auto md:hidden">
                    {repoDetailsSections.map((section) => (
                      <Button
                        key={section.id}
                        type="button"
                        size="sm"
                        variant={selectedRepoSection === section.id ? 'secondary' : 'ghost'}
                        onClick={() => setSelectedRepoSection(section.id)}
                      >
                        <AppIcon
                          icon={section.icon}
                          stroke={1.5}
                          aria-hidden
                          data-icon="inline-start"
                        />
                        {section.label}
                      </Button>
                    ))}
                  </div>
                </header>

                <form className="flex min-h-0 flex-1 flex-col" onSubmit={handleUpdateRepo}>
                  <div className="flex-1 overflow-y-auto p-6">
                    {selectedRepoSection === 'overview' ? (
                      <RepoOverviewSection
                        repository={selectedRepo}
                        serviceAccount={serviceAccountById.get(selectedRepo.service_account_id)}
                        runs={runs.filter((run) => run.repository_id === selectedRepo.id)}
                      />
                    ) : null}

                    {selectedRepoSection === 'settings' ? (
                      <div className="flex flex-col gap-4">
                        <FieldGroup>
                          <Field>
                            <FieldLabel htmlFor="manage-service-account">
                              Service account
                            </FieldLabel>
                            <Select
                              value={updateForm.service_account_id}
                              onValueChange={(value) =>
                                setUpdateForm((current) =>
                                  current ? { ...current, service_account_id: value } : current,
                                )
                              }
                            >
                              <SelectTrigger id="manage-service-account">
                                <SelectValue placeholder="Select service account" />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectGroup>
                                  {serviceAccounts.map((account) => (
                                    <SelectItem key={account.id} value={account.id}>
                                      {formatServiceAccount(account)}
                                    </SelectItem>
                                  ))}
                                </SelectGroup>
                              </SelectContent>
                            </Select>
                            <FieldDescription>
                              Action API keys must belong to this service account. Rebinding does
                              not revoke existing keys.
                            </FieldDescription>
                          </Field>
                        </FieldGroup>

                        <div className="flex flex-col gap-3">
                          {featureToggles.map((toggle) => (
                            <SettingToggleRow
                              key={toggle.key}
                              label={toggle.label}
                              description={toggle.description}
                              value={updateForm.settings[toggle.key]}
                              onChange={(value) =>
                                setUpdateForm((current) =>
                                  current
                                    ? {
                                        ...current,
                                        settings: { ...current.settings, [toggle.key]: value },
                                      }
                                    : current,
                                )
                              }
                            />
                          ))}

                          <SettingToggleRow
                            label="Request changes on high severity"
                            description="Submit the review as request-changes when high-severity findings exist. Findings never fail PR checks."
                            value={updateForm.settings.request_changes_on_high_severity}
                            onChange={(value) =>
                              setUpdateForm((current) =>
                                current
                                  ? {
                                      ...current,
                                      settings: {
                                        ...current.settings,
                                        request_changes_on_high_severity: value,
                                      },
                                    }
                                  : current,
                              )
                            }
                          />
                        </div>

                        <FieldGroup>
                          <Field>
                            <FieldLabel htmlFor="manage-max-comments">
                              Max inline comments
                            </FieldLabel>
                            <Input
                              id="manage-max-comments"
                              type="number"
                              min={1}
                              value={updateForm.settings.max_inline_comments ?? ''}
                              onChange={(event) =>
                                setUpdateForm((current) =>
                                  current
                                    ? {
                                        ...current,
                                        settings: {
                                          ...current.settings,
                                          max_inline_comments:
                                            event.target.value === ''
                                              ? null
                                              : Number(event.target.value),
                                        },
                                      }
                                    : current,
                                )
                              }
                              placeholder="No limit"
                            />
                            <FieldDescription>
                              Caps inline findings per review. Leave empty for no limit.
                            </FieldDescription>
                          </Field>

                          <Field>
                            <FieldLabel htmlFor="manage-default-model">
                              Default model key
                            </FieldLabel>
                            <Input
                              id="manage-default-model"
                              value={updateForm.settings.default_model_key ?? ''}
                              onChange={(event) =>
                                setUpdateForm((current) =>
                                  current
                                    ? {
                                        ...current,
                                        settings: {
                                          ...current.settings,
                                          default_model_key: event.target.value || null,
                                        },
                                      }
                                    : current,
                                )
                              }
                              placeholder="Use per-run action input"
                            />
                            <FieldDescription>
                              Oceans model key used when the workflow does not pass a model-id
                              input. Reviews are skipped when neither is set.
                            </FieldDescription>
                          </Field>
                        </FieldGroup>

                        <section className="flex flex-col gap-3 rounded-lg border border-[color:var(--color-border)] p-4">
                          <div className="flex flex-col gap-1">
                            <h3 className="text-sm font-semibold text-[var(--color-text)]">
                              Lifecycle actions
                            </h3>
                            <p className="text-sm text-[var(--color-text-muted)]">
                              Disabling stops new review runs immediately without deleting run
                              history. Historical runs stay visible.
                            </p>
                          </div>

                          <div className="flex flex-wrap gap-2">
                            {selectedRepo.status === 'active' ? (
                              <Button
                                type="button"
                                variant="destructive"
                                onClick={handleDisableRepo}
                                disabled={isPending}
                              >
                                Disable
                              </Button>
                            ) : (
                              <Button
                                type="button"
                                variant="secondary"
                                onClick={handleReactivateRepo}
                                disabled={isPending}
                              >
                                Reactivate
                              </Button>
                            )}
                          </div>
                        </section>
                      </div>
                    ) : null}

                    {selectedRepoSection === 'setup' ? (
                      <div className="flex flex-col gap-4">
                        <Alert>
                          <AlertTitle>Finish setup in the repository</AlertTitle>
                          <AlertDescription>
                            Create an API key for the bound service account, store it as the
                            workflow secret below, then commit the generated workflow file. The
                            workflow only reviews same-repository, non-draft pull requests and never
                            uses pull_request_target.
                          </AlertDescription>
                        </Alert>

                        <FieldGroup>
                          <div className="grid gap-4 sm:grid-cols-2">
                            <Field>
                              <FieldLabel htmlFor="workflow-action-ref">Action ref</FieldLabel>
                              <Input
                                id="workflow-action-ref"
                                value={workflowForm.action_ref}
                                onChange={(event) =>
                                  setWorkflowForm((current) => ({
                                    ...current,
                                    action_ref: event.target.value,
                                  }))
                                }
                                placeholder="main"
                              />
                              <FieldDescription>
                                Branch, tag, or SHA of ahstn/oceans-llm to pin the action to.
                              </FieldDescription>
                            </Field>

                            <Field>
                              <FieldLabel htmlFor="workflow-secret-name">
                                API key secret name
                              </FieldLabel>
                              <Input
                                id="workflow-secret-name"
                                value={workflowForm.api_key_secret_name}
                                onChange={(event) =>
                                  setWorkflowForm((current) => ({
                                    ...current,
                                    api_key_secret_name: event.target.value,
                                  }))
                                }
                                placeholder="OCEANS_API_KEY"
                              />
                              <FieldDescription>
                                GitHub Actions secret holding the service account API key.
                              </FieldDescription>
                            </Field>
                          </div>

                          <div>
                            <Button
                              type="button"
                              variant="secondary"
                              onClick={generateWorkflow}
                              disabled={isWorkflowPending}
                            >
                              {isWorkflowPending
                                ? 'Generating…'
                                : workflow
                                  ? 'Regenerate workflow'
                                  : 'Generate workflow'}
                            </Button>
                          </div>
                        </FieldGroup>

                        {workflowError ? (
                          <Alert>
                            <AlertTitle>Workflow generation failed</AlertTitle>
                            <AlertDescription>{workflowError}</AlertDescription>
                          </Alert>
                        ) : null}

                        {workflow ? (
                          <div className="flex flex-col gap-3">
                            <Field>
                              <FieldLabel htmlFor="workflow-file-name">Workflow file</FieldLabel>
                              <InputGroup>
                                <InputGroupInput
                                  id="workflow-file-name"
                                  readOnly
                                  value={`.github/workflows/${workflow.file_name}`}
                                />
                                <InputGroupAddon align="inline-end">
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    onClick={() =>
                                      handleCopy(
                                        `.github/workflows/${workflow.file_name}`,
                                        'File path copied',
                                      )
                                    }
                                  >
                                    Copy
                                  </Button>
                                </InputGroupAddon>
                              </InputGroup>
                            </Field>

                            <div className="flex flex-col gap-2">
                              <div className="flex items-center justify-between">
                                <p className="text-sm font-semibold text-[var(--color-text)]">
                                  Generated workflow
                                </p>
                                <Button
                                  type="button"
                                  variant="secondary"
                                  size="sm"
                                  onClick={() => handleCopy(workflow.yaml, 'Workflow YAML copied')}
                                >
                                  Copy YAML
                                </Button>
                              </div>
                              <pre className="max-h-72 overflow-auto rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4 font-mono text-xs leading-relaxed text-[var(--color-text)]">
                                {workflow.yaml}
                              </pre>
                            </div>

                            <ol className="flex list-decimal flex-col gap-1 pl-5 text-sm text-[var(--color-text-muted)]">
                              <li>
                                Create an API key for the bound service account under API Keys.
                              </li>
                              <li>
                                Add it to {selectedRepo.full_name} as the repository secret{' '}
                                <code>{workflow.api_key_secret_name}</code>.
                              </li>
                              <li>
                                Commit the YAML above to{' '}
                                <code>.github/workflows/{workflow.file_name}</code> on the default
                                branch.
                              </li>
                            </ol>
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                  </div>

                  <DialogFooter className="mx-0 mb-0 rounded-none border-t border-[color:var(--color-border)] px-6 py-4">
                    <Button type="button" variant="secondary" onClick={closeRepoDialog}>
                      Close
                    </Button>
                    <Button type="submit" disabled={isPending}>
                      {isPending ? 'Saving…' : 'Save changes'}
                    </Button>
                  </DialogFooter>
                </form>
              </main>
            </SidebarProvider>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  )
}

function RepoOverviewSection({
  repository,
  serviceAccount,
  runs,
}: {
  repository: ReviewAgentRepositoryView
  serviceAccount: ServiceAccountView | undefined
  runs: ReviewAgentRunView[]
}) {
  const lastRun = runs[0] ?? null
  const commentsPosted = runs.reduce((sum, run) => sum + (run.inline_comments_created ?? 0), 0)
  const linesReviewed = runs.reduce((sum, run) => sum + (run.changed_loc ?? 0), 0)

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h3 className="text-sm font-semibold text-[var(--color-text)]">Repository</h3>
        <dl className="mt-5 grid gap-x-8 gap-y-6 text-sm sm:grid-cols-2 lg:grid-cols-3">
          <RepoDetailRow label="Provider" value={formatProvider(repository.provider)} />
          <RepoDetailRow label="Owner" value={repository.owner} />
          <RepoDetailRow label="Name" value={repository.name} />
          <RepoDetailRow
            label="Service account"
            value={serviceAccount ? formatServiceAccount(serviceAccount) : '—'}
          />
          <RepoDetailRow label="Status" value={repository.status} />
          <RepoDetailRow label="Configured" value={formatTimestamp(repository.created_at)} />
        </dl>
      </div>

      <div>
        <h3 className="text-sm font-semibold text-[var(--color-text)]">Recent activity</h3>
        <div className="mt-4 grid gap-4 md:grid-cols-3">
          <section className="rounded-lg border border-[color:var(--color-border)] p-4">
            <p className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
              Last review
            </p>
            <p className="mt-2 text-lg font-semibold text-[var(--color-text)]">
              {lastRun ? lastRun.status : 'Never'}
            </p>
            <p className="mt-1 text-sm text-[var(--color-text-muted)]">
              {lastRun ? formatTimestamp(runTimestamp(lastRun)) : 'No runs reported yet.'}
            </p>
          </section>
          <section className="rounded-lg border border-[color:var(--color-border)] p-4">
            <p className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
              Recent runs
            </p>
            <p className="mt-2 text-lg font-semibold text-[var(--color-text)]">{runs.length}</p>
            <p className="mt-1 text-sm text-[var(--color-text-muted)]">
              {commentsPosted} inline comments posted across recent runs.
            </p>
          </section>
          <section className="rounded-lg border border-[color:var(--color-border)] p-4">
            <p className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
              Lines reviewed
            </p>
            <p className="mt-2 text-lg font-semibold text-[var(--color-text)]">
              {linesReviewed.toLocaleString()}
            </p>
            <p className="mt-1 text-sm text-[var(--color-text-muted)]">
              Changed lines covered by recent review runs.
            </p>
          </section>
        </div>
      </div>
    </div>
  )
}

function SettingToggleRow({
  label,
  description,
  value,
  onChange,
}: {
  label: string
  description: string
  value: boolean
  onChange: (value: boolean) => void
}) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-lg border border-[color:var(--color-border)] p-4">
      <div className="flex flex-col gap-1">
        <p className="text-sm font-semibold text-[var(--color-text)]">{label}</p>
        <p className="text-sm text-[var(--color-text-muted)]">{description}</p>
      </div>
      <ToggleGroup
        type="single"
        value={value ? 'on' : 'off'}
        onValueChange={(next) => {
          if (next === 'on' || next === 'off') {
            onChange(next === 'on')
          }
        }}
        aria-label={label}
      >
        <ToggleGroupItem value="on" aria-label={`${label} on`}>
          On
        </ToggleGroupItem>
        <ToggleGroupItem value="off" aria-label={`${label} off`}>
          Off
        </ToggleGroupItem>
      </ToggleGroup>
    </div>
  )
}

function RepoDetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase">
        {label}
      </dt>
      <dd className="mt-1 text-[var(--color-text-muted)]">{value}</dd>
    </div>
  )
}

function normalizeReviewAgentSearch(search: Record<string, unknown>) {
  const section = typeof search.repo_section === 'string' ? search.repo_section : 'overview'

  return {
    repo_id:
      typeof search.repo_id === 'string' && search.repo_id.length > 0 ? search.repo_id : undefined,
    repo_section: isRepoDetailsSection(section) ? section : 'overview',
  }
}

function isRepoDetailsSection(value: string): value is RepoDetailsSection {
  return repoDetailsSections.some((section) => section.id === value)
}

function sanitizeSettings(settings: ReviewAgentSettingsView): ReviewAgentSettingsView {
  return {
    ...settings,
    default_model_key: settings.default_model_key?.trim() || null,
    max_inline_comments:
      settings.max_inline_comments && settings.max_inline_comments > 0
        ? Math.floor(settings.max_inline_comments)
        : null,
  }
}

function formatServiceAccount(account: ServiceAccountView) {
  return `${account.name} (${account.team_name})`
}

function formatProvider(provider: string) {
  return provider === 'github' ? 'GitHub' : provider
}

function formatFeatureSummary(settings: ReviewAgentSettingsView) {
  const enabled = featureToggles.filter((toggle) => settings[toggle.key]).length
  return `${enabled} of ${featureToggles.length} enabled`
}

function repoStatusVariant(status: string) {
  if (status === 'active') {
    return 'success' as const
  }
  return status === 'disabled' ? ('warning' as const) : ('default' as const)
}

function runStatusVariant(status: string) {
  switch (status) {
    case 'succeeded':
      return 'success' as const
    case 'failed':
      return 'destructive' as const
    case 'queued':
    case 'in_progress':
      return 'warning' as const
    default:
      return 'secondary' as const
  }
}

function runTimestamp(run: ReviewAgentRunView) {
  return run.finished_at ?? run.started_at ?? run.created_at
}

function formatTimestamp(value: string | null | undefined) {
  if (!value) {
    return '—'
  }
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '—' : date.toLocaleString()
}

function formatLineDelta(run: ReviewAgentRunView) {
  if (run.additions == null && run.deletions == null) {
    return '—'
  }
  return `+${run.additions ?? 0} / −${run.deletions ?? 0}`
}

function formatDuration(durationMs: number | null | undefined) {
  if (durationMs == null) {
    return '—'
  }
  if (durationMs < 1_000) {
    return `${durationMs}ms`
  }
  const seconds = Math.round(durationMs / 1_000)
  if (seconds < 60) {
    return `${seconds}s`
  }
  return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, '0')}s`
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Something went wrong'
}
