import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  ReviewAgentOverviewPayload,
  ReviewAgentRepositoryView,
  ReviewAgentRunView,
} from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
  navigate: vi.fn(async () => {}),
}

const createRepoMock = vi.fn()
const renderWorkflowMock = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => routerMock,
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  createReviewAgentRepo: (...args: unknown[]) => createRepoMock(...args),
  disableReviewAgentRepo: vi.fn(),
  getReviewAgentOverview: vi.fn(),
  reactivateReviewAgentRepo: vi.fn(),
  renderReviewAgentRepoWorkflow: (...args: unknown[]) => renderWorkflowMock(...args),
  updateReviewAgentRepo: vi.fn(),
}))

const oceansRepo: ReviewAgentRepositoryView = {
  id: 'repo_oceans',
  provider: 'github',
  owner: 'ahstn',
  name: 'oceans-llm',
  full_name: 'ahstn/oceans-llm',
  external_repository_id: '910128343',
  service_account_id: 'svc_review_bot',
  settings: {
    inline_review_enabled: true,
    pr_summary_enabled: true,
    diagrams_enabled: false,
    linked_issue_detection_enabled: true,
    linked_issue_assessment_enabled: false,
    max_inline_comments: 20,
    default_model_key: 'reasoning',
    request_changes_on_high_severity: false,
  },
  settings_json: null,
  status: 'active',
  created_at: '2026-06-28T09:00:00Z',
  updated_at: '2026-07-02T09:00:00Z',
}

const dotfilesRepo: ReviewAgentRepositoryView = {
  id: 'repo_dotfiles',
  provider: 'github',
  owner: 'ahstn',
  name: 'dotfiles',
  full_name: 'ahstn/dotfiles',
  external_repository_id: null,
  service_account_id: 'svc_review_bot',
  settings: {
    inline_review_enabled: true,
    pr_summary_enabled: true,
    diagrams_enabled: false,
    linked_issue_detection_enabled: true,
    linked_issue_assessment_enabled: false,
    max_inline_comments: null,
    default_model_key: null,
    request_changes_on_high_severity: false,
  },
  settings_json: null,
  status: 'disabled',
  created_at: '2026-06-29T09:00:00Z',
  updated_at: '2026-07-01T09:00:00Z',
}

const seedRuns: ReviewAgentRunView[] = [
  {
    id: 'run_1',
    repository_id: 'repo_oceans',
    status: 'succeeded',
    head_sha: 'abc1234def5678',
    github_run_id: '16400001',
    github_run_attempt: 1,
    model_key: 'reasoning',
    model_execution_mode: 'oceans',
    files_changed: 12,
    additions: 240,
    deletions: 80,
    changed_loc: 320,
    inline_comments_created: 4,
    linked_issue_count: 1,
    duration_ms: 92_000,
    effective_config_json: {},
    started_at: '2026-07-02T08:58:00Z',
    finished_at: '2026-07-02T09:00:00Z',
    created_at: '2026-07-02T08:58:00Z',
    updated_at: '2026-07-02T09:00:00Z',
  },
  {
    id: 'run_2',
    repository_id: 'repo_dotfiles',
    status: 'failed',
    head_sha: 'fed9876cba4321',
    model_key: 'fast',
    error_summary: 'GitHub token missing pull request write permission',
    effective_config_json: {},
    started_at: '2026-07-01T08:00:00Z',
    finished_at: '2026-07-01T08:01:00Z',
    created_at: '2026-07-01T08:00:00Z',
    updated_at: '2026-07-01T08:01:00Z',
  },
]

const seedPayload: ReviewAgentOverviewPayload = {
  repositories: [oceansRepo, dotfilesRepo],
  service_accounts: [
    {
      id: 'svc_review_bot',
      key: 'review-bot',
      name: 'Review Bot',
      status: 'active',
      team_id: 'team_platform',
      team_key: 'platform',
      team_name: 'Platform',
    },
  ],
  runs: seedRuns,
}

const emptyPayload: ReviewAgentOverviewPayload = {
  repositories: [],
  service_accounts: [],
  runs: [],
}

describe('ReviewAgentPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReturnValue({ repo_id: undefined, repo_section: 'overview' })
    routerMock.invalidate.mockClear()
    routerMock.navigate.mockClear()
    createRepoMock.mockReset()
    renderWorkflowMock.mockReset()
  })

  it('teaches the next step when no repositories are configured', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: emptyPayload })

    const { ReviewAgentPage } = await import('@/routes/review-agent')

    render(<ReviewAgentPage />)

    expect(screen.getByText('No repositories configured yet')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Configure first repository' }))

    expect(screen.getByText('No service accounts available')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Configure repository' })).toBeDisabled()
  })

  it('lists configured repositories with status and recent runs', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: seedPayload })

    const { ReviewAgentPage } = await import('@/routes/review-agent')

    render(<ReviewAgentPage />)

    expect(screen.getAllByText('ahstn/oceans-llm').length).toBeGreaterThan(0)
    expect(screen.getAllByText('ahstn/dotfiles').length).toBeGreaterThan(0)
    expect(screen.getAllByText('disabled').length).toBeGreaterThan(0)
    expect(screen.getAllByText('3 of 5 enabled').length).toBeGreaterThan(0)

    expect(screen.getByText('abc1234')).toBeInTheDocument()
    expect(screen.getByText('+240 / −80')).toBeInTheDocument()
    expect(screen.getByText('1m 32s')).toBeInTheDocument()
  })

  it('shows review settings toggles and lifecycle actions in the manage dialog', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: seedPayload })
    routeMock.useSearch.mockReturnValue({ repo_id: 'repo_oceans', repo_section: 'settings' })

    const { ReviewAgentPage } = await import('@/routes/review-agent')

    render(<ReviewAgentPage />)

    expect(screen.getByText('Inline review')).toBeInTheDocument()
    expect(screen.getByText('Linked issue assessment')).toBeInTheDocument()
    expect(screen.getByLabelText('Max inline comments')).toHaveValue(20)
    expect(screen.getByLabelText('Default model key')).toHaveValue('reasoning')
    expect(screen.getByText('Lifecycle actions')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Disable' })).toBeInTheDocument()
  })

  it('generates and displays the copy/paste workflow in the setup section', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: seedPayload })
    routeMock.useSearch.mockReturnValue({ repo_id: 'repo_oceans', repo_section: 'setup' })
    renderWorkflowMock.mockResolvedValue({
      data: {
        yaml: 'name: Oceans Review Agent\non: pull_request',
        file_name: 'oceans-review-agent.yml',
        action_ref: 'main',
        api_key_secret_name: 'OCEANS_API_KEY',
        oceans_url: 'https://oceans.example.test',
      },
    })

    const { ReviewAgentPage } = await import('@/routes/review-agent')

    render(<ReviewAgentPage />)

    await waitFor(() => expect(renderWorkflowMock).toHaveBeenCalledTimes(1))
    expect(renderWorkflowMock).toHaveBeenCalledWith({
      data: {
        repositoryId: 'repo_oceans',
        input: { action_ref: null, api_key_secret_name: null },
      },
    })

    await waitFor(() => expect(screen.getByText(/name: Oceans Review Agent/)).toBeInTheDocument())

    expect(screen.queryByLabelText('Workflow file')).not.toBeInTheDocument()
    expect(screen.getByText('.github/workflows/oceans-review-agent.yml')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy YAML' })).toBeInTheDocument()
  })
})
