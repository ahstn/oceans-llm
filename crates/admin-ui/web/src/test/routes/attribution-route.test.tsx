import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type React from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import { AttributionPage } from '@/routes/observability/attribution'
import { ATTRIBUTION_DOCS_URL } from '@/routes/observability/-attribution/shared'
import type { AttributionReportView } from '@/types/api'

// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useSearch: vi.fn(),
}))
const navigateMock = vi.fn()
const invalidateMock = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock, invalidate: invalidateMock }),
  // Tabs are links; the test reads which one is current, not where it goes.
  Link: ({
    children,
    to: _to,
    search: _search,
    ...props
  }: {
    children: React.ReactNode
    to?: string
    search?: unknown
  } & Record<string, unknown>) => (
    <a href="#tab" {...props}>
      {children}
    </a>
  ),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getAttribution: vi.fn(),
  // The Usage Costs helpers the page reuses import this one.
  getSpendUsageReport: vi.fn(),
}))

// Attribution user refs carry no email.
const alice = { id: 'user-alice', name: 'Alice Doe' }
const bob = { id: 'user-bob', name: 'Bob Roe' }

function periodSummary(): AttributionReportView['summary'] {
  return {
    commit_count: 3,
    merged_pull_request_count: 1,
    ai_lines: 120,
    human_lines: 40,
    merged_ai_lines: 100,
    merged_human_lines: 20,
    priced_cost_usd_10000: 138_400,
    shipped_cost_usd_10000: 100_000,
    median_pull_request_cost_usd_10000: 123_400,
    median_lead_time_seconds: 104_400,
    reverted_pull_request_count: 0,
  }
}

function attributionReport(overrides: Partial<AttributionReportView> = {}): AttributionReportView {
  return {
    window_days: 30,
    window_start: '2026-10-13T00:00:00Z',
    window_end: '2026-11-12T00:00:00Z',
    user_id: null,
    repository: null,
    totals: {
      commit_count: 3,
      pull_request_count: 1,
      ai_lines: 120,
      human_lines: 40,
      ai_session_count: 3,
      joined_session_count: 2,
      agent_session_count: 2,
      request_count: 42,
      priced_request_count: 40,
      unpriced_request_count: 2,
      usage_missing_request_count: 0,
      priced_cost_usd_10000: 123_400,
      uncommitted_request_count: 5,
      uncommitted_cost_usd_10000: 15_000,
      unmatched_author_commit_count: 1,
    },
    coverage: {
      // Coverage counts every request, so it includes the 5 uncommitted ones.
      request_count: 47,
      priced_request_count: 45,
      unpriced_request_count: 2,
      usage_missing_request_count: 0,
      priced_cost_usd_10000: 138_400,
      commits_truncated: false,
      pull_requests_truncated: false,
      agent_sessions_truncated: false,
      request_facts_truncated: false,
    },
    users: [
      {
        user: alice,
        commit_count: 2,
        merged_pull_request_count: 1,
        ai_lines: 100,
        human_lines: 20,
        ai_session_count: 2,
        agent_session_count: 2,
        request_count: 40,
        priced_request_count: 40,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 123_400,
        models: ['claude-sonnet-5'],
        last_activity_at: '2026-11-10T12:00:00Z',
      },
      {
        user: bob,
        commit_count: 1,
        merged_pull_request_count: 0,
        ai_lines: 20,
        human_lines: 20,
        ai_session_count: 1,
        agent_session_count: 0,
        request_count: 2,
        priced_request_count: 0,
        unpriced_request_count: 2,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 0,
        models: ['local-model'],
        last_activity_at: '2026-11-09T12:00:00Z',
      },
    ],
    models: [
      {
        model_key: 'claude-sonnet-5',
        request_count: 40,
        priced_request_count: 40,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 123_400,
        commit_count: 2,
        uncommitted_request_count: 5,
        uncommitted_cost_usd_10000: 15_000,
      },
      {
        model_key: 'local-model',
        request_count: 2,
        priced_request_count: 0,
        unpriced_request_count: 2,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 0,
        commit_count: 1,
        uncommitted_request_count: 0,
        uncommitted_cost_usd_10000: 0,
      },
    ],
    pull_requests: [
      {
        repository: 'github.com/acme/widgets',
        number: 42,
        title: 'Add widget search',
        url: 'https://github.com/acme/widgets/pull/42',
        base_ref: 'main',
        head_ref: 'widget-search',
        merged_at: '2026-11-10T15:00:00Z',
        author: alice,
        commit_count: 2,
        attributed_commit_count: 2,
        ai_lines: 100,
        human_lines: 20,
        request_count: 40,
        priced_request_count: 40,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 123_400,
        models: ['claude-sonnet-5'],
        first_commit_at: '2026-11-09T10:00:00Z',
        lead_time_seconds: 104_400,
        is_revert: false,
        agent_tools: ['claude'],
        commits: [
          {
            commit_sha: 'abcdef1234567890abcdef1234567890abcdef12',
            committed_at: '2026-11-10T12:00:00Z',
            ai_lines: 80,
            human_lines: 10,
            agent_tools: ['claude'],
            models: ['claude-sonnet-5'],
            request_count: 30,
            priced_request_count: 30,
            unpriced_request_count: 0,
            usage_missing_request_count: 0,
            priced_cost_usd_10000: 100_000,
          },
        ],
      },
    ],
    daily: [],
    session_outcomes: [],
    harness_models: [],
    summary: periodSummary(),
    previous_summary: periodSummary(),
    previous_truncated: false,
    changes: [
      {
        attribution_commit_id: 'commit-1',
        repository: 'github.com/acme/widgets',
        commit_sha: 'abcdef1234567890abcdef1234567890abcdef12',
        branch: 'widget-search',
        is_default_branch: false,
        committed_at: '2026-11-10T12:00:00Z',
        author: alice,
        owner: { kind: 'user', id: alice.id, name: alice.name },
        pull_request_number: 42,
        ai_lines: 80,
        human_lines: 10,
        session_count: 1,
        joined_session_count: 1,
        unjoined_session_count: 0,
        request_count: 30,
        priced_request_count: 30,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 100_000,
        models: ['claude-sonnet-5'],
        note_models: ['claude-sonnet-5'],
        agent_tools: ['claude'],
      },
      {
        attribution_commit_id: 'commit-2',
        repository: 'github.com/acme/widgets',
        commit_sha: '1234567fedcba9876543210fedcba9876543210f',
        branch: 'main',
        is_default_branch: true,
        committed_at: '2026-11-09T12:00:00Z',
        author: null,
        owner: { kind: 'user', id: bob.id, name: bob.name },
        pull_request_number: null,
        ai_lines: 20,
        human_lines: 20,
        session_count: 1,
        joined_session_count: 0,
        unjoined_session_count: 1,
        request_count: 2,
        priced_request_count: 0,
        unpriced_request_count: 2,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 0,
        models: [],
        note_models: ['local-model'],
        agent_tools: ['pi'],
      },
    ],
    ...overrides,
  }
}

function emptyReport(overrides: Partial<AttributionReportView> = {}): AttributionReportView {
  const report = attributionReport()
  return {
    ...report,
    totals: {
      ...report.totals,
      commit_count: 0,
      pull_request_count: 0,
      ai_lines: 0,
      human_lines: 0,
      ai_session_count: 0,
      joined_session_count: 0,
      agent_session_count: 0,
      request_count: 0,
      priced_request_count: 0,
      unpriced_request_count: 0,
      priced_cost_usd_10000: 0,
      uncommitted_request_count: 0,
      uncommitted_cost_usd_10000: 0,
      unmatched_author_commit_count: 0,
    },
    coverage: {
      ...report.coverage,
      request_count: 0,
      priced_request_count: 0,
      unpriced_request_count: 0,
      priced_cost_usd_10000: 0,
    },
    users: [],
    models: [],
    pull_requests: [],
    changes: [],
    ...overrides,
  }
}

const ALL_TABS = ['impact', 'delivery', 'pull_requests', 'repositories', 'durability', 'my_work']

function session(globalRole: 'platform_admin' | 'user', attributionTabs: string[] = ALL_TABS) {
  return {
    session: {
      must_change_password: false,
      user: { id: alice.id, name: alice.name, email: 'alice@example.com', global_role: globalRole },
      capabilities: { attribution_tabs: attributionTabs },
    },
  }
}

function renderPage(report: AttributionReportView, search: Record<string, unknown> = { days: 30 }) {
  routeMock.useLoaderData.mockReturnValue({ data: report, meta: {} })
  routeMock.useSearch.mockReturnValue(search)
  render(
    <TooltipProvider>
      <AttributionPage />
    </TooltipProvider>,
  )
}

/** The KPI card whose label is `label`; the label sits in the card description. */
function kpi(label: string) {
  const labelNode = screen
    .getAllByText(label)
    .find((node) => node.closest('[data-slot="card-description"]'))
  return within(labelNode!.closest<HTMLElement>('[data-slot="card"]')!)
}

beforeEach(() => {
  routeMock.useLoaderData.mockReset()
  routeMock.useRouteContext.mockReset()
  routeMock.useSearch.mockReset()
  navigateMock.mockReset()
  invalidateMock.mockReset()
  routeMock.useRouteContext.mockReturnValue(session('platform_admin'))
})

afterEach(() => {
  cleanup()
})

function tabNav() {
  return within(screen.getByRole('navigation', { name: 'Attribution sections' }))
}

describe('AttributionPage', () => {
  it('opens on Impact with the headline figures against the previous window', async () => {
    renderPage(attributionReport())

    expect(screen.getByRole('heading', { level: 1, name: 'Attribution' })).toBeInTheDocument()
    expect(tabNav().getByRole('link', { name: 'Impact' })).toHaveAttribute('aria-current', 'page')
    // Merged code: 100 AI and 20 human lines.
    expect((await screen.findAllByText('AI share of merged code')).length).toBeGreaterThan(0)
    expect(kpi('AI share of merged code').getByText('83%')).toBeInTheDocument()
    expect(kpi('Cost per merged PR').getByText('$12.34')).toBeInTheDocument()
    expect(kpi('Merged PRs').getByText('3 commits')).toBeInTheDocument()

    const users = within(screen.getByRole('table', { name: 'User activity' }))
    const aliceRow = users.getByRole('row', { name: /Alice Doe/ })
    expect(within(aliceRow).getByText('$12.34')).toBeInTheDocument()
    expect(within(aliceRow).getByText('2 hooked')).toBeInTheDocument()
    // Durability is not collected yet, so its counterweight carries the pill.
    expect(screen.getAllByText('Needs new data').length).toBeGreaterThan(0)
  })

  it('shows only the tabs the gateway config turns on', async () => {
    routeMock.useRouteContext.mockReturnValue(session('platform_admin', ['delivery', 'my_work']))
    renderPage(attributionReport())

    const links = tabNav().getAllByRole('link')
    expect(links.map((link) => link.textContent)).toEqual(['Delivery', 'My work'])
    // Impact is off, so the page opens on the first tab that is on.
    expect(tabNav().getByRole('link', { name: 'Delivery' })).toHaveAttribute('aria-current', 'page')
    expect(await screen.findByText('Lead time for changes')).toBeInTheDocument()
    expect(tabNav().queryByRole('link', { name: 'Concepts' })).not.toBeInTheDocument()
  })

  it('explains how to turn tabs back on when every tab is off', () => {
    routeMock.useRouteContext.mockReturnValue(session('platform_admin', []))
    renderPage(attributionReport())

    expect(screen.getByText('Every Attribution tab is turned off')).toBeInTheDocument()
    expect(
      screen.queryByRole('navigation', { name: 'Attribution sections' }),
    ).not.toBeInTheDocument()
  })

  it('lists merged pull requests and recent changes on the Pull requests tab', async () => {
    renderPage(attributionReport(), { days: 30, tab: 'pull_requests' })

    const pullRequests = within(await screen.findByRole('table', { name: 'Merged pull requests' }))
    expect(pullRequests.getByRole('link', { name: 'github.com/acme/widgets#42' })).toHaveAttribute(
      'href',
      'https://github.com/acme/widgets/pull/42',
    )
    expect(pullRequests.getByText('Add widget search')).toBeInTheDocument()
    expect(pullRequests.getByText('1/2')).toBeInTheDocument()

    fireEvent.click(pullRequests.getByRole('button', { name: 'Expand #42' }))
    const commits = within(screen.getByRole('table', { name: 'Commits in #42' }))
    expect(commits.getByText('abcdef1')).toBeInTheDocument()
    expect(commits.getByText('$10.00')).toBeInTheDocument()

    const changes = within(screen.getByRole('table', { name: 'Recent changes' }))
    const joinedRow = changes.getByRole('row', { name: /abcdef1/ })
    expect(within(joinedRow).getByText('1 joined')).toBeInTheDocument()
    expect(within(joinedRow).getByRole('link', { name: '#42' })).toHaveAttribute(
      'href',
      'https://github.com/acme/widgets/pull/42',
    )
    const unjoinedRow = changes.getByRole('row', { name: /1234567/ })
    expect(within(unjoinedRow).getByText('1 unjoined')).toBeInTheDocument()
    expect(within(unjoinedRow).getByText('Unmatched author')).toBeInTheDocument()
    expect(within(unjoinedRow).getByText('Key owner: Bob Roe')).toBeInTheDocument()
    expect(within(unjoinedRow).getByText('Default')).toBeInTheDocument()
    // No joined requests, so the models come from the note.
    expect(within(unjoinedRow).getByText('local-model')).toBeInTheDocument()
    expect(within(unjoinedRow).getAllByRole('cell')[10]).toHaveTextContent('—')
    expect(screen.getByText('Showing 2 of 2 changes')).toBeInTheDocument()
  })

  it('renders unpriced cost as — rather than $0', async () => {
    renderPage(attributionReport())

    const bobRow = within(await screen.findByRole('table', { name: 'User activity' })).getByRole(
      'row',
      { name: /Bob Roe/ },
    )
    const bobCost = within(bobRow).getAllByRole('cell')[7]
    expect(bobCost).toHaveTextContent('2 gaps')
    expect(bobCost).toHaveTextContent('—')
    expect(screen.queryByText('$0.00')).not.toBeInTheDocument()
  })

  it('filters to a user from the name button in their row', async () => {
    renderPage(attributionReport())
    const users = within(await screen.findByRole('table', { name: 'User activity' }))

    const aliceButton = users.getByRole('button', { name: 'Filter the report to Alice Doe' })
    expect(aliceButton).toHaveAttribute('aria-pressed', 'false')
    expect(users.getByRole('row', { name: /Alice Doe/ })).not.toHaveAttribute('tabindex')
    fireEvent.click(aliceButton)
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/attribution',
        search: { days: 30, user_id: 'user-alice' },
      })
    })
    expect(navigateMock).toHaveBeenCalledTimes(1)
  })

  it('clears the user filter from the header', async () => {
    renderPage(attributionReport({ user_id: alice.id }), { days: 30, user_id: alice.id })

    const users = within(await screen.findByRole('table', { name: 'User activity' }))
    expect(users.getByRole('row', { name: /Alice Doe/ })).toHaveAttribute('aria-current', 'true')
    expect(users.getByRole('button', { name: 'Filter the report to Alice Doe' })).toHaveAttribute(
      'aria-pressed',
      'true',
    )
    fireEvent.click(screen.getByRole('button', { name: 'Clear user filter: Alice Doe' }))
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/attribution',
        search: { days: 30 },
      })
    })
  })

  it('offers no user filter on My work, which is always your own report', async () => {
    renderPage(attributionReport({ user_id: alice.id }), {
      days: 30,
      tab: 'my_work',
      user_id: alice.id,
    })

    expect(await screen.findByText('Your AI share')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Clear user filter/ })).not.toBeInTheDocument()
    expect(screen.getByText(/compared with your previous 30 days/)).toBeInTheDocument()
  })

  it('offers no user filter to a user who only sees their own activity', async () => {
    routeMock.useRouteContext.mockReturnValue(session('user'))
    renderPage(attributionReport())

    const users = within(await screen.findByRole('table', { name: 'User activity' }))
    expect(users.getByRole('row', { name: /Alice Doe/ })).toBeInTheDocument()
    expect(users.queryByRole('button', { name: /Filter the report to/ })).not.toBeInTheDocument()
  })

  it('keeps the needs-new-data reminders on the Durability tab', async () => {
    renderPage(attributionReport(), { days: 30, tab: 'durability' })

    expect(await screen.findByText('No survival data yet')).toBeInTheDocument()
    expect(screen.getAllByText('Needs new data').length).toBeGreaterThanOrEqual(5)
    expect(screen.getByRole('link', { name: 'the attribution data gaps note' })).toHaveAttribute(
      'href',
      expect.stringContaining('docs/internal/2026-11-15-attribution-data-gaps.md'),
    )
  })

  it('changes the report window', async () => {
    renderPage(attributionReport(), { days: 30, repository: 'github.com/acme/widgets' })

    fireEvent.click(screen.getByRole('radio', { name: 'Last 7 days' }))
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/attribution',
        search: { days: 7, repository: 'github.com/acme/widgets' },
      })
    })
  })

  it('explains the hook install when there is no attribution data', () => {
    renderPage(emptyReport())

    expect(screen.getByText('No attributed commits yet')).toBeInTheDocument()
    expect(screen.getByText('oceans hook install --git-ai --claude')).toBeInTheDocument()
    expect(screen.getByText('oceans hook install --git-ai --omp')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Set up Git AI attribution' })).toHaveAttribute(
      'href',
      ATTRIBUTION_DOCS_URL,
    )
    expect(screen.queryByRole('table')).not.toBeInTheDocument()
  })

  it('explains the hook install to a user whose own id the gateway echoes', () => {
    routeMock.useRouteContext.mockReturnValue(session('user'))
    renderPage(emptyReport({ user_id: alice.id }))

    expect(screen.getByText('No attributed commits yet')).toBeInTheDocument()
    expect(screen.queryByText('No attribution matches these filters')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Clear filters' })).not.toBeInTheDocument()
  })

  it('offers to clear filters when nothing matches them', async () => {
    renderPage(emptyReport({ repository: 'github.com/acme/other' }), {
      days: 90,
      repository: 'github.com/acme/other',
    })

    expect(screen.getByText('No attribution matches these filters')).toBeInTheDocument()
    expect(screen.queryByText('No attributed commits yet')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/attribution',
        search: { days: 90 },
      })
    })
  })

  it('hides changes against a previous window that hit a read limit', async () => {
    renderPage(attributionReport({ previous_truncated: true }))

    expect(screen.getByText('No comparison with the previous window')).toBeInTheDocument()
    expect((await screen.findAllByText('AI share of merged code')).length).toBeGreaterThan(0)
    expect(kpi('Merged PRs').getByText('No prior period')).toBeInTheDocument()
  })

  it('leaves pull requests without notes out of the AI-share bands', async () => {
    const report = attributionReport()
    const withoutNotes = {
      ...report.pull_requests[0],
      number: 43,
      title: 'Bump dependencies',
      ai_lines: 0,
      human_lines: 0,
      attributed_commit_count: 0,
      commits: [],
    }
    renderPage(attributionReport({ pull_requests: [report.pull_requests[0], withoutNotes] }), {
      days: 30,
      tab: 'delivery',
    })

    const bands = within(await screen.findByRole('table', { name: 'Delivery by AI share' }))
    const noAi = bands.getByRole('row', { name: /^No AI/ })
    expect(within(noAi).getAllByRole('cell')[1]).toHaveTextContent('0')
    expect(screen.getByText(/1 merged pull requests have no notes/)).toBeInTheDocument()
  })

  it('warns when the report hit a read limit', () => {
    const report = attributionReport()
    renderPage(attributionReport({ coverage: { ...report.coverage, commits_truncated: true } }))

    expect(screen.getByText('This report is incomplete')).toBeInTheDocument()
  })

  it('warns when the agent session lookup hit its read limit', () => {
    const report = attributionReport()
    renderPage(
      attributionReport({ coverage: { ...report.coverage, agent_sessions_truncated: true } }),
    )

    expect(screen.getByText('This report is incomplete')).toBeInTheDocument()
  })
})
