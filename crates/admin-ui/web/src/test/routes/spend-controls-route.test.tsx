import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const routeMock = {
  useLoaderData: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({
    invalidate: vi.fn(async () => {}),
  }),
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

describe('SpendControlsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
  })

  it('renders user and team budget management tables', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        users: [
          {
            user_id: 'user_1',
            name: 'Jane Admin',
            email: 'jane@example.com',
            team_id: null,
            team_name: null,
            budget: {
              cadence: 'daily',
              amount_usd: '100.0000',
              amount_usd_10000: 1_000_000,
              hard_limit: true,
              timezone: 'UTC',
            },
            current_window_spend_usd_10000: 125_000,
          },
        ],
        teams: [
          {
            team_id: 'team_1',
            team_name: 'Core Platform',
            team_key: 'core-platform',
            budget: null,
            current_window_spend_usd_10000: 0,
          },
        ],
      },
    })

    const { SpendControlsPage } = await import('@/routes/spend-controls')
    render(<SpendControlsPage />)

    expect(screen.getByText('Spend Controls')).toBeInTheDocument()
    expect(screen.getByText('Jane Admin')).toBeInTheDocument()
    expect(screen.getByText('Core Platform')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Configure' }).length).toBeGreaterThan(0)
  })
})
