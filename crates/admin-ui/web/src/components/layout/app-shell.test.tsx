import type { ReactNode } from 'react'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AppShell } from '@/components/layout/app-shell'

vi.mock('@tanstack/react-router', async () => ({
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
  useRouterState: () => '/admin/api-keys',
}))

describe('AppShell', () => {
  it('renders all required menu sections and items', () => {
    render(<AppShell>content</AppShell>)

    const labels = [
      'API Keys',
      'Models',
      'Observability',
      'Usage Costs',
      'Request Logs',
      'Identity Management',
      'Teams',
      'Users',
    ]

    for (const label of labels) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0)
    }
  })
})
