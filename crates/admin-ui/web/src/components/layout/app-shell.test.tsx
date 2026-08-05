import type { ReactNode } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AppShell } from '@/components/layout/app-shell'
import { TooltipProvider } from '@/components/ui/tooltip'
import { platformAdminSession, regularUserSession } from '@/test/auth-session'

vi.mock('@tanstack/react-router', async () => ({
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
  useRouterState: () => '/admin/api-keys',
}))

const logoutAdminSession = vi.fn()

vi.mock('@/server/admin-data.functions', () => ({
  logoutAdminSession: () => logoutAdminSession(),
}))

describe('AppShell', () => {
  const originalLocation = window.location

  beforeEach(() => {
    logoutAdminSession.mockReset()
    logoutAdminSession.mockResolvedValue({ data: { status: 'ok' } })
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...originalLocation, replace: vi.fn() },
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
      expect(window.location.replace).toHaveBeenCalledWith('/admin/login')
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
