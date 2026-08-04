import type { ReactNode } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AppShell } from '@/components/layout/app-shell'
import { TooltipProvider } from '@/components/ui/tooltip'

let routerPath = '/admin/api-keys'

vi.mock('@tanstack/react-router', async () => ({
  Link: ({ children, to }: { children: ReactNode; to: string }) => <a href={to}>{children}</a>,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => string }) =>
    select({ location: { pathname: routerPath } }),
}))

const logoutAdminSession = vi.fn()

vi.mock('@/server/admin-data.functions', () => ({
  logoutAdminSession: () => logoutAdminSession(),
}))

describe('AppShell', () => {
  const originalLocation = window.location

  beforeEach(() => {
    routerPath = '/admin/api-keys'
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
        <AppShell
          oceansVersion="0.17.0"
          session={{
            must_change_password: false,
            user: {
              id: 'user_1',
              name: 'Admin User',
              email: 'admin@example.com',
              global_role: 'platform_admin',
            },
          }}
        >
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
        <AppShell
          oceansVersion={null}
          session={{
            must_change_password: false,
            user: {
              id: 'user_1',
              name: 'Admin User',
              email: 'admin@example.com',
              global_role: 'platform_admin',
            },
          }}
        >
          content
        </AppShell>
      </TooltipProvider>,
    )

    expect(screen.getByText('Oceans')).toBeVisible()
    expect(screen.queryByText(/^Oceans v/)).not.toBeInTheDocument()
  })

  it('limits a regular user to the connection page', () => {
    routerPath = '/admin/account/connections'
    render(
      <TooltipProvider>
        <AppShell
          oceansVersion="0.17.0"
          session={{
            must_change_password: false,
            user: {
              id: 'user_2',
              name: 'Workspace User',
              email: 'user@example.com',
              global_role: 'user',
            },
          }}
        >
          content
        </AppShell>
      </TooltipProvider>,
    )

    expect(screen.getAllByText('Account').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Connections').length).toBeGreaterThan(0)
    expect(screen.getByRole('link', { name: 'Account' })).toHaveAttribute(
      'href',
      '/account/connections',
    )
    expect(screen.queryByText('Control Plane')).not.toBeInTheDocument()
    expect(screen.queryByText('Models')).not.toBeInTheDocument()
    expect(screen.queryByText('Identity')).not.toBeInTheDocument()
  })

  it('signs out from the account menu', async () => {
    render(
      <TooltipProvider>
        <AppShell
          oceansVersion="0.17.0"
          session={{
            must_change_password: false,
            user: {
              id: 'user_1',
              name: 'Admin User',
              email: 'admin@example.com',
              global_role: 'platform_admin',
            },
          }}
        >
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
})
