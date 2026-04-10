import type { ReactNode } from 'react'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AppShell } from '@/components/layout/app-shell'
import { TooltipProvider } from '@/components/ui/tooltip'

vi.mock('@tanstack/react-router', async () => ({
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
  useRouterState: () => '/admin/api-keys',
}))

describe('AppShell', () => {
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
    ]

    for (const label of labels) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0)
    }
  })
})
