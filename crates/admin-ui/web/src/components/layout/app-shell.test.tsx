import type { ReactNode } from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AppShell } from '@/components/layout/app-shell'
import { TooltipProvider } from '@/components/ui/tooltip'

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
        <AppShell
          session={{
            must_change_password: false,
            user: {
              id: 'user_1',
              name: 'Admin User',
              email: 'admin@example.com',
              global_role: 'owner',
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
      'Identity',
      'Admin User',
      'admin@example.com',
    ]

    for (const label of labels) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0)
    }
  })

  it('signs out from the account menu', async () => {
    render(
      <TooltipProvider>
        <AppShell
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
