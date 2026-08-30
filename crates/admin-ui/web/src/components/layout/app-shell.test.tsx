import type { ReactNode } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AppShell } from '@/components/layout/app-shell'
import { TooltipProvider } from '@/components/ui/tooltip'
import { platformAdminSession, regularUserSession } from '@/test/auth-session'

let routerPath = '/admin/api-keys'

vi.mock('@tanstack/react-router', async () => ({
  Link: ({ children, to }: { children: ReactNode; to: string }) => <a href={to}>{children}</a>,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => string }) =>
    select({ location: { pathname: routerPath } }),
}))

const logoutAdminSession = vi.fn()
const replaceLocation = vi.fn()

vi.mock('@/server/admin-data.functions', () => ({
  logoutAdminSession: () => logoutAdminSession(),
}))

describe('AppShell', () => {
  const originalLocation = window.location

  beforeEach(() => {
    routerPath = '/admin/api-keys'
    logoutAdminSession.mockReset()
    logoutAdminSession.mockResolvedValue({ data: { status: 'ok' } })
    replaceLocation.mockReset()
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { href: originalLocation.href, replace: replaceLocation },
    })
  })

  afterEach(() => {
    cleanup()
  })

  it('renders all required menu sections and items', () => {
    render(
      <TooltipProvider>
        <AppShell oceansVersion="0.17.0" session={platformAdminSession()}>
          content
        </AppShell>
      </TooltipProvider>,
    )

    const labels = [
      'API Keys',
      'Models',
      'Control Plane',
      'Observability',
      'MCP Invocations',
      'Identity',
      'Admin User',
      'admin@example.com',
      'Oceans v0.17.0',
    ]

    for (const label of labels) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0)
    }

    expect(screen.queryByText('Server-first · same-origin')).not.toBeInTheDocument()
  })

  it('renders an unversioned fallback when gateway version is unavailable', () => {
    render(
      <TooltipProvider>
        <AppShell oceansVersion={null} session={platformAdminSession()}>
          content
        </AppShell>
      </TooltipProvider>,
    )

    expect(screen.getByText('Oceans')).toBeVisible()
    expect(screen.queryByText(/^Oceans v/)).not.toBeInTheDocument()
  })

  it('shows the connection page within regular-user navigation', () => {
    routerPath = '/admin/account/connections'
    render(
      <TooltipProvider>
        <AppShell oceansVersion="0.17.0" session={regularUserSession()}>
          content
        </AppShell>
      </TooltipProvider>,
    )

    expect(screen.getAllByText('Connections').length).toBeGreaterThan(0)
    expect(screen.getByRole('link', { name: 'Control Plane' })).toHaveAttribute('href', '/api-keys')
    expect(screen.getAllByText('Models').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Identity').length).toBeGreaterThan(0)
  })

  it('signs out from the account menu', async () => {
    render(
      <TooltipProvider>
        <AppShell oceansVersion="0.17.0" session={platformAdminSession()}>
          content
        </AppShell>
      </TooltipProvider>,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: /Admin User/i }))
    expect(await screen.findByText('Change password')).toBeVisible()
    expect(screen.getByText('Platform Admin')).toBeVisible()

    fireEvent.click(screen.getByText('Sign out'))

    await waitFor(() => {
      expect(logoutAdminSession).toHaveBeenCalledTimes(1)
      expect(replaceLocation).toHaveBeenCalledWith('/admin/login')
    })
  })

  it('shows self-service credentials, models, and observability links to regular users', () => {
    render(
      <TooltipProvider>
        <AppShell oceansVersion="0.17.0" session={regularUserSession()}>
          content
        </AppShell>
      </TooltipProvider>,
    )

    expect(screen.getByText('Usage Costs')).toBeVisible()
    expect(screen.getByText('Request Logs')).toBeVisible()
    expect(screen.getByText('MCP Invocations')).toBeVisible()
    expect(screen.getByText('Connections')).toBeVisible()
    expect(screen.getAllByText('API Keys').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Models').length).toBeGreaterThan(0)
    expect(screen.getByText('Teams')).toBeVisible()
    expect(screen.getByText('Users')).toBeVisible()
    expect(screen.getByText('Identity')).toBeVisible()
    expect(screen.getByText('Leaderboard')).toBeVisible()
    expect(screen.getByText('Agent Harnesses')).toBeVisible()
    expect(screen.getByText('Service Accounts')).toBeVisible()
  })

  it('hides pages that are absent from the resolved permission set', () => {
    render(
      <TooltipProvider>
        <AppShell oceansVersion="0.17.0" session={regularUserSession(['models'])}>
          content
        </AppShell>
      </TooltipProvider>,
    )

    expect(screen.getByText('Models')).toBeVisible()
    expect(screen.queryByText('API Keys')).not.toBeInTheDocument()
    expect(screen.queryByText('Identity')).not.toBeInTheDocument()
  })
})
