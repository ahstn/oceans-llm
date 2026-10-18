import { formatCount } from '@/lib/format'
import {
  Configuration01Icon,
  SourceCodeIcon,
  WorkflowSquare01Icon,
} from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import type { ReviewAgentRunView, ReviewAgentSettingsView, ServiceAccountView } from '@/types/api'

export const repoDetailsSections = [
  { id: 'overview', label: 'Overview', icon: SourceCodeIcon },
  { id: 'settings', label: 'Review settings', icon: Configuration01Icon },
  { id: 'setup', label: 'Workflow setup', icon: WorkflowSquare01Icon },
] as const

export type RepoDetailsSection = (typeof repoDetailsSections)[number]['id']

export const defaultSettings: ReviewAgentSettingsView = {
  inline_review_enabled: true,
  pr_summary_enabled: true,
  diagrams_enabled: false,
  linked_issue_detection_enabled: true,
  linked_issue_assessment_enabled: false,
  max_inline_comments: null,
  default_model_key: null,
  request_changes_on_high_severity: false,
}

export const featureToggles = [
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

export function normalizeReviewAgentSearch(search: Record<string, unknown>) {
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

export function sanitizeSettings(settings: ReviewAgentSettingsView): ReviewAgentSettingsView {
  return {
    ...settings,
    default_model_key: settings.default_model_key?.trim() || null,
    max_inline_comments:
      settings.max_inline_comments && settings.max_inline_comments > 0
        ? Math.floor(settings.max_inline_comments)
        : null,
  }
}

export function formatServiceAccount(account: ServiceAccountView) {
  return `${account.name} (${account.team_name})`
}

export function formatProvider(provider: string) {
  return provider === 'github' ? 'GitHub' : provider
}

export function formatFeatureSummary(settings: ReviewAgentSettingsView) {
  const enabled = featureToggles.filter((toggle) => settings[toggle.key]).length
  return `${enabled} of ${featureToggles.length} enabled`
}

export function repoStatusVariant(status: string) {
  if (status === 'active') {
    return 'success' as const
  }
  // Disabled is inactive, not pending, as for users and service accounts.
  return 'secondary' as const
}

export function runStatusVariant(status: string) {
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

export function runTimestamp(run: ReviewAgentRunView) {
  return run.finished_at ?? run.started_at ?? run.created_at
}

export function formatLineDelta(run: ReviewAgentRunView) {
  if (run.additions == null && run.deletions == null) {
    return '—'
  }
  return `+${formatCount(run.additions ?? 0)} / −${formatCount(run.deletions ?? 0)}`
}

export function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Something went wrong'
}

export async function copyToClipboard(value: string, message: string) {
  try {
    await navigator.clipboard.writeText(value)
    toast.success(message)
  } catch {
    toast.error('Clipboard access failed')
  }
}
