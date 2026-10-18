import {
  useEffect,
  useEffectEvent,
  useState,
  useTransition,
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent,
} from 'react'
import { Cancel01Icon } from '@hugeicons/core-free-icons'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { SectionNavSidebar, SectionNavTabs } from '@/components/shared/section-nav'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { Input } from '@/components/ui/input'
import { SidebarProvider } from '@/components/ui/sidebar'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { formatTimestamp } from '@/lib/format'
import {
  disableReviewAgentRepo,
  reactivateReviewAgentRepo,
  renderReviewAgentRepoWorkflow,
  updateReviewAgentRepo,
} from '@/server/admin-data.functions'
import type {
  ReviewAgentRepositoryView,
  ReviewAgentRunView,
  ReviewAgentSettingsView,
  ReviewAgentWorkflowPayload,
  ServiceAccountView,
} from '@/types/api'

import {
  copyToClipboard,
  defaultSettings,
  featureToggles,
  formatProvider,
  formatServiceAccount,
  getErrorMessage,
  repoDetailsSections,
  repoStatusVariant,
  runTimestamp,
  sanitizeSettings,
  type RepoDetailsSection,
} from './review-agent-meta'
import { ServiceAccountSelect } from './service-account-select'

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

interface RepoDetailsDialogProps {
  repository: ReviewAgentRepositoryView | null
  section: RepoDetailsSection
  serviceAccounts: ServiceAccountView[]
  runs: ReviewAgentRunView[]
  onSectionChange: (section: RepoDetailsSection) => void
  onClose: () => void
}

/** The `Manage repository` dialog, open while the URL selects a repository. */
export function RepoDetailsDialog({ repository, onClose, ...props }: RepoDetailsDialogProps) {
  return (
    <Dialog
      open={Boolean(repository)}
      onOpenChange={(open) => {
        if (!open) {
          onClose()
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

        {repository ? (
          // Keyed by repository so the forms start from that repository's saved values.
          <RepoDetailsPanel
            key={repository.id}
            repository={repository}
            onClose={onClose}
            {...props}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function RepoDetailsPanel({
  repository,
  section,
  serviceAccounts,
  runs,
  onSectionChange,
  onClose,
}: Omit<RepoDetailsDialogProps, 'repository'> & { repository: ReviewAgentRepositoryView }) {
  const router = useRouter()
  const [form, setForm] = useState<RepoUpdateForm>(() => ({
    service_account_id: repository.service_account_id,
    settings: { ...defaultSettings, ...repository.settings },
  }))
  const [isPending, startTransition] = useTransition()
  const workflow = useRepoWorkflow(repository.id, section === 'setup')
  const serviceAccount = serviceAccounts.find(
    (account) => account.id === repository.service_account_id,
  )

  function runAction(action: () => Promise<unknown>, message: string) {
    startTransition(async () => {
      try {
        await action()
        toast.success(message)
        await router.invalidate()
        onClose()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    runAction(
      () =>
        updateReviewAgentRepo({
          data: {
            repositoryId: repository.id,
            input: {
              external_repository_id: repository.external_repository_id ?? null,
              full_name: repository.full_name,
              name: repository.name,
              owner: repository.owner,
              service_account_id: form.service_account_id,
              settings: sanitizeSettings(form.settings),
              settings_json: repository.settings_json,
              status: repository.status,
            },
          },
        }),
      'Repository updated',
    )
  }

  return (
    <SidebarProvider
      className="min-h-0 min-w-0 items-start overflow-hidden"
      style={{ '--sidebar-width': '14rem' } as CSSProperties}
    >
      <SectionNavSidebar
        sections={repoDetailsSections}
        activeSection={section}
        onSectionChange={onSectionChange}
      />

      <main className="flex max-h-[680px] min-h-[520px] min-w-0 flex-1 flex-col overflow-hidden">
        <RepoDetailsHeader
          repository={repository}
          serviceAccountName={serviceAccount?.name}
          section={section}
          onSectionChange={onSectionChange}
        />

        <form className="flex min-h-0 flex-1 flex-col" onSubmit={handleSubmit}>
          <div className="flex-1 overflow-y-auto p-6">
            {section === 'overview' ? (
              <RepoOverviewSection
                repository={repository}
                serviceAccount={serviceAccount}
                runs={runs.filter((run) => run.repository_id === repository.id)}
              />
            ) : null}

            {section === 'settings' ? (
              <RepoSettingsSection
                repository={repository}
                serviceAccounts={serviceAccounts}
                form={form}
                isPending={isPending}
                onFormChange={setForm}
                onDisable={() =>
                  runAction(
                    () => disableReviewAgentRepo({ data: { repositoryId: repository.id } }),
                    'Repository disabled',
                  )
                }
                onReactivate={() =>
                  runAction(
                    () => reactivateReviewAgentRepo({ data: { repositoryId: repository.id } }),
                    'Repository reactivated',
                  )
                }
              />
            ) : null}

            {section === 'setup' ? (
              <WorkflowSetupSection repository={repository} workflow={workflow} />
            ) : null}
          </div>

          <DialogFooter className="border-border mx-0 mb-0 rounded-none border-t px-6 py-4">
            <Button type="button" variant="secondary" onClick={onClose}>
              Close
            </Button>
            <Button type="submit" disabled={isPending}>
              {isPending ? 'Saving…' : 'Save changes'}
            </Button>
          </DialogFooter>
        </form>
      </main>
    </SidebarProvider>
  )
}

function RepoDetailsHeader({
  repository,
  serviceAccountName,
  section,
  onSectionChange,
}: {
  repository: ReviewAgentRepositoryView
  serviceAccountName: string | undefined
  section: RepoDetailsSection
  onSectionChange: (section: RepoDetailsSection) => void
}) {
  return (
    <header className="border-border flex shrink-0 flex-col gap-4 border-b px-6 py-5">
      <div className="flex items-start gap-3">
        <GeneratedAvatar kind="team" name={repository.full_name} size={44} />
        <div className="min-w-0 flex-1 pt-0.5">
          <h2 className="text-foreground truncate text-lg leading-tight font-semibold">
            {repository.full_name}
          </h2>
          <p className="text-subtle-foreground mt-1 truncate text-sm">
            {formatProvider(repository.provider)} ·{' '}
            {serviceAccountName ?? 'Unknown service account'}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Badge variant={repoStatusVariant(repository.status)}>{repository.status}</Badge>
          <DialogClose asChild>
            <Button type="button" variant="ghost" size="icon-sm" aria-label="Close">
              <AppIcon icon={Cancel01Icon} stroke={1.5} aria-hidden />
            </Button>
          </DialogClose>
        </div>
      </div>

      <SectionNavTabs
        sections={repoDetailsSections}
        activeSection={section}
        onSectionChange={onSectionChange}
      />
    </header>
  )
}

/** Workflow inputs and the rendered YAML, generated once the setup section first opens. */
function useRepoWorkflow(repositoryId: string, active: boolean) {
  const [form, setForm] = useState<WorkflowForm>(initialWorkflowForm)
  const [result, setResult] = useState<ReviewAgentWorkflowPayload | null>(null)
  const [isPending, setIsPending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function generate(input: WorkflowForm) {
    setIsPending(true)
    setError(null)
    try {
      const response = await renderReviewAgentRepoWorkflow({
        data: {
          repositoryId,
          input: {
            action_ref: input.action_ref.trim() || null,
            api_key_secret_name: input.api_key_secret_name.trim() || null,
          },
        },
      })
      setResult(response.data)
    } catch (caught) {
      setError(getErrorMessage(caught))
    } finally {
      setIsPending(false)
    }
  }

  const generateInitial = useEffectEvent(() => {
    void generate(initialWorkflowForm)
  })

  useEffect(() => {
    if (active && !result && !isPending && !error) {
      generateInitial()
    }
  }, [active, result, isPending, error])

  return { form, setForm, result, isPending, error, generate }
}

function RepoSettingsSection({
  repository,
  serviceAccounts,
  form,
  isPending,
  onFormChange,
  onDisable,
  onReactivate,
}: {
  repository: ReviewAgentRepositoryView
  serviceAccounts: ServiceAccountView[]
  form: RepoUpdateForm
  isPending: boolean
  onFormChange: (form: RepoUpdateForm) => void
  onDisable: () => void
  onReactivate: () => void
}) {
  function setSetting<Key extends keyof ReviewAgentSettingsView>(
    key: Key,
    value: ReviewAgentSettingsView[Key],
  ) {
    onFormChange({ ...form, settings: { ...form.settings, [key]: value } })
  }

  return (
    <div className="flex flex-col gap-4">
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="manage-service-account">Service account</FieldLabel>
          <ServiceAccountSelect
            id="manage-service-account"
            value={form.service_account_id}
            serviceAccounts={serviceAccounts}
            onValueChange={(value) => onFormChange({ ...form, service_account_id: value })}
          />
          <FieldDescription>
            Action API keys must belong to this service account. Rebinding does not revoke existing
            keys.
          </FieldDescription>
        </Field>
      </FieldGroup>

      <div className="divide-border border-border flex flex-col divide-y border-y">
        {featureToggles.map((toggle) => (
          <SettingToggleRow
            key={toggle.key}
            label={toggle.label}
            description={toggle.description}
            value={form.settings[toggle.key]}
            onChange={(value) => setSetting(toggle.key, value)}
          />
        ))}

        <SettingToggleRow
          label="Request changes on high severity"
          description="Submit the review as request-changes when high-severity findings exist. Findings never fail PR checks."
          value={form.settings.request_changes_on_high_severity}
          onChange={(value) => setSetting('request_changes_on_high_severity', value)}
        />
      </div>

      <FieldGroup>
        <Field>
          <FieldLabel htmlFor="manage-max-comments">Max inline comments</FieldLabel>
          <Input
            id="manage-max-comments"
            type="number"
            min={1}
            value={form.settings.max_inline_comments ?? ''}
            onChange={(event) =>
              setSetting(
                'max_inline_comments',
                event.target.value === '' ? null : Number(event.target.value),
              )
            }
            placeholder="No limit"
          />
          <FieldDescription>
            Caps inline findings per review. Leave empty for no limit.
          </FieldDescription>
        </Field>

        <Field>
          <FieldLabel htmlFor="manage-default-model">Default model key</FieldLabel>
          <Input
            id="manage-default-model"
            value={form.settings.default_model_key ?? ''}
            onChange={(event) => setSetting('default_model_key', event.target.value || null)}
            placeholder="Use per-run action input"
          />
          <FieldDescription>
            Oceans model key used when the workflow does not pass a model-id input. Reviews are
            skipped when neither is set.
          </FieldDescription>
        </Field>
      </FieldGroup>

      <LifecycleActions
        status={repository.status}
        isPending={isPending}
        onDisable={onDisable}
        onReactivate={onReactivate}
      />
    </div>
  )
}

/** Disable or reactivate reviews for the repository; run history is kept either way. */
function LifecycleActions({
  status,
  isPending,
  onDisable,
  onReactivate,
}: {
  status: string
  isPending: boolean
  onDisable: () => void
  onReactivate: () => void
}) {
  return (
    <section className="border-border flex flex-col gap-3 rounded-lg border p-4">
      <div className="flex flex-col gap-1">
        <h3 className="text-foreground text-sm font-semibold">Lifecycle actions</h3>
        <p className="text-subtle-foreground text-sm">
          Disabling stops new review runs immediately without deleting run history. Historical runs
          stay visible.
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        {status === 'active' ? (
          <Button type="button" variant="destructive" onClick={onDisable} disabled={isPending}>
            Disable
          </Button>
        ) : (
          <Button type="button" variant="secondary" onClick={onReactivate} disabled={isPending}>
            Reactivate
          </Button>
        )}
      </div>
    </section>
  )
}

function WorkflowSetupSection({
  repository,
  workflow,
}: {
  repository: ReviewAgentRepositoryView
  workflow: ReturnType<typeof useRepoWorkflow>
}) {
  const { form, setForm, result, isPending, error, generate } = workflow

  // Enter regenerates the workflow instead of submitting the surrounding settings form.
  function handleInputKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key !== 'Enter') {
      return
    }

    event.preventDefault()
    void generate(form)
  }

  return (
    <div className="flex min-w-0 flex-col gap-4 overflow-hidden">
      <Alert className="min-w-0 overflow-hidden">
        <AlertTitle>Finish setup in the repository</AlertTitle>
        <AlertDescription>
          Create an API key for the bound service account, store it as the workflow secret below,
          then commit the generated workflow file. The workflow only reviews same-repository,
          non-draft pull requests and never uses pull_request_target.
        </AlertDescription>
      </Alert>

      <FieldGroup className="min-w-0 overflow-hidden">
        <div className="grid min-w-0 gap-4 sm:grid-cols-2">
          <Field className="min-w-0">
            <FieldLabel htmlFor="workflow-action-ref">Action ref</FieldLabel>
            <Input
              id="workflow-action-ref"
              value={form.action_ref}
              onKeyDown={handleInputKeyDown}
              onChange={(event) => setForm({ ...form, action_ref: event.target.value })}
              placeholder="main"
            />
            <FieldDescription>
              Branch, tag, or SHA of ahstn/oceans-llm to pin the action to.
            </FieldDescription>
          </Field>

          <Field className="min-w-0">
            <FieldLabel htmlFor="workflow-secret-name">API key secret name</FieldLabel>
            <Input
              id="workflow-secret-name"
              value={form.api_key_secret_name}
              onKeyDown={handleInputKeyDown}
              onChange={(event) => setForm({ ...form, api_key_secret_name: event.target.value })}
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
            onClick={() => void generate(form)}
            disabled={isPending}
          >
            {isPending ? 'Generating…' : result ? 'Regenerate workflow' : 'Generate workflow'}
          </Button>
        </div>
      </FieldGroup>

      {error ? (
        <Alert>
          <AlertTitle>Workflow generation failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}

      {result ? (
        <div className="flex min-w-0 flex-col gap-3 overflow-hidden">
          <div className="flex min-w-0 flex-col gap-2">
            <div className="flex min-w-0 items-center justify-between gap-3">
              <p className="text-foreground min-w-0 truncate text-sm font-semibold">
                Generated workflow
              </p>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => copyToClipboard(result.yaml, 'Workflow YAML copied')}
              >
                Copy YAML
              </Button>
            </div>
            <pre className="border-border bg-surface-muted text-foreground max-h-72 max-w-full min-w-0 overflow-auto rounded-lg border p-4 font-mono text-xs leading-relaxed whitespace-pre">
              {result.yaml}
            </pre>
          </div>

          <ol className="text-subtle-foreground flex list-decimal flex-col gap-1 pl-5 text-sm">
            <li>Create an API key for the bound service account under API Keys.</li>
            <li>
              Add it to {repository.full_name} as the repository secret{' '}
              <code>{result.api_key_secret_name}</code>.
            </li>
            <li>
              Commit the YAML above to <code>.github/workflows/{result.file_name}</code> on the
              default branch.
            </li>
          </ol>
        </div>
      ) : null}
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
        <h3 className="text-foreground text-sm font-semibold">Repository</h3>
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
        <h3 className="text-foreground text-sm font-semibold">Recent activity</h3>
        <div className="mt-4 grid gap-4 md:grid-cols-3">
          <section className="border-border rounded-lg border p-4">
            <p className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
              Last review
            </p>
            <p className="text-foreground mt-2 text-lg font-semibold">
              {lastRun ? lastRun.status : 'Never'}
            </p>
            <p className="text-subtle-foreground mt-1 text-sm">
              {lastRun ? formatTimestamp(runTimestamp(lastRun)) : 'No runs reported yet.'}
            </p>
          </section>
          <section className="border-border rounded-lg border p-4">
            <p className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
              Recent runs
            </p>
            <p className="text-foreground mt-2 text-lg font-semibold">{runs.length}</p>
            <p className="text-subtle-foreground mt-1 text-sm">
              {commentsPosted} inline comments posted across recent runs.
            </p>
          </section>
          <section className="border-border rounded-lg border p-4">
            <p className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
              Lines reviewed
            </p>
            <p className="text-foreground mt-2 text-lg font-semibold">
              {linesReviewed.toLocaleString()}
            </p>
            <p className="text-subtle-foreground mt-1 text-sm">
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
    <div className="flex items-start justify-between gap-4 py-4">
      <div className="flex min-w-0 flex-col gap-1">
        <p className="text-foreground text-sm font-semibold">{label}</p>
        <p className="text-subtle-foreground text-sm">{description}</p>
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
        className="border-border shrink-0 overflow-hidden rounded-lg border"
      >
        <ToggleGroupItem value="on" aria-label={`${label} on`} className="border-border border-r">
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
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className="text-subtle-foreground mt-1">{value}</dd>
    </div>
  )
}
