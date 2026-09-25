import type { ReactNode } from 'react'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthSessionView } from '@/types/api'
import { apiKey, apiKeysPayload, profileView } from '@/test/profile-fixtures'

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
}

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  Link: ({
    to,
    search,
    children,
    ...props
  }: {
    to: string
    search?: Record<string, unknown>
    children: ReactNode
  }) => {
    const query = new URLSearchParams(
      Object.entries(search ?? {}).map(([key, value]) => [key, String(value)]),
    ).toString()
    return (
      <a href={query ? `${to}?${query}` : to} {...props}>
        {children}
      </a>
    )
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  getApiKeys: vi.fn(),
  getMyProfile: vi.fn(),
}))

function session(overrides: Partial<AuthSessionView['permissions']> = {}): AuthSessionView {
  return {
    must_change_password: false,
    capabilities: {
      agent_analysis: false,
      calibrated_score_visible: false,
      passive_analysis_enabled: false,
      platform_admin: false,
      shadow_diagnostics_visible: false,
      team_admin_analytics_enabled: false,
    },
    permissions: {
      group: 'users',
      pages: ['api_keys'],
      actions: ['create_api_key'],
      default_page: 'api_keys',
      ...overrides,
    },
    user: { id: 'user_1', name: 'Jane User', email: 'jane@example.com', global_role: 'user' },
  } as AuthSessionView
}

const FIVE_KEYS = ['a', 'b', 'c', 'd', 'e'].map((id) => apiKey(id))

const load = async () => (await import('@/routes/profile/index')).ProfileOverviewPage

describe('profile page', () => {
  beforeEach(() => {
    cleanup()
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    routeMock.useLoaderData.mockReturnValue({
      profile: profileView(),
      keys: apiKeysPayload(FIVE_KEYS),
    })
    routeMock.useRouteContext.mockReturnValue({ session: session() })
  })

  it('shows budget used against the limit and the headline figures', async () => {
    const Page = await load()
    render(<Page />)

    const meter = screen.getByTestId('budget-meter')
    expect(meter).toHaveTextContent('$25.00')
    expect(meter).toHaveTextContent('of $100.00')
    expect(meter).toHaveTextContent('$75.00 remaining')
    const headlines = screen.getByTestId('profile-headlines')
    expect(headlines).toHaveTextContent('reasoning')
    expect(headlines).toHaveTextContent('Claude Code')
    expect(headlines).toHaveTextContent('Cache hit rate')
  })

  it('links to the API keys page from the page header', async () => {
    const Page = await load()
    render(<Page />)

    const header = screen.getByRole('heading', { name: 'Welcome back, Jane' }).closest('header')
    expect(header).not.toBeNull()
    expect(within(header!).getByRole('link', { name: 'Manage keys' })).toHaveAttribute(
      'href',
      '/api-keys',
    )
  })

  it('limits the key table to three rows until expanded', async () => {
    const Page = await load()
    render(<Page />)

    expect(screen.getByText('Key a')).toBeInTheDocument()
    expect(screen.queryByText('Key d')).toBeNull()

    const toggle = screen.getByRole('button', { name: 'Show 2 more' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(toggle)

    expect(screen.getByText('Key e')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Show fewer' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
  })

  it('links the empty state to the create key dialog', async () => {
    routeMock.useLoaderData.mockReturnValue({ profile: profileView(), keys: apiKeysPayload([]) })
    const Page = await load()
    render(<Page />)

    expect(screen.getByText('No personal API keys yet')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Create your first key' })).toHaveAttribute(
      'href',
      '/api-keys?create=true',
    )
  })

  it('hides the create link from users who cannot create keys', async () => {
    routeMock.useLoaderData.mockReturnValue({ profile: profileView(), keys: apiKeysPayload([]) })
    routeMock.useRouteContext.mockReturnValue({ session: session({ actions: [] }) })
    const Page = await load()
    render(<Page />)

    expect(screen.queryByRole('link', { name: 'Create your first key' })).toBeNull()
    expect(screen.getByText(/Ask a platform admin/)).toBeInTheDocument()
  })

  it('shows requests, tokens and cost for a hovered heatmap day', async () => {
    const Page = await load()
    render(<Page />)

    const heatmap = screen.getByTestId('usage-heatmap')
    const day = within(heatmap).getByRole('img', { name: /Fri, Sep 25, 2026/ })
    expect(day).toHaveAttribute('data-level', '4')
    fireEvent.pointerOver(day)

    const tooltip = screen.getByTestId('heatmap-tooltip')
    expect(tooltip).toHaveTextContent('Fri, Sep 25, 2026')
    expect(tooltip).toHaveTextContent('Requests10')
    expect(tooltip).toHaveTextContent('Total tokens9,000')
    expect(tooltip).toHaveTextContent('Cost$2.50')

    fireEvent.pointerLeave(heatmap)
    expect(screen.queryByTestId('heatmap-tooltip')).toBeNull()
  })

  it('explains a missing budget instead of showing an empty meter', async () => {
    routeMock.useLoaderData.mockReturnValue({
      profile: profileView({ budget: null }),
      keys: apiKeysPayload(FIVE_KEYS),
    })
    const Page = await load()
    render(<Page />)

    expect(screen.queryByTestId('budget-meter')).toBeNull()
    expect(screen.getByTestId('no-budget')).toBeInTheDocument()
  })
})
