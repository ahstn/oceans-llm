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

  it('renders user, service account, and user model budget management tables', async () => {
    routeMock.useLoaderData.mockReturnValue({
      budgets: {
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
              alert_email_ready: true,
              alert_recipient_summary: 'jane@example.com',
            },
          ],
          service_accounts: [
            {
              service_account_id: 'service_account_1',
              service_account_name: 'CI Indexer',
              service_account_key: 'ci-indexer',
              team_id: 'team_1',
              team_name: 'Core Platform',
              team_key: 'core-platform',
              budget: null,
              current_window_spend_usd_10000: 0,
              alert_email_ready: false,
              alert_recipient_summary: 'No active team owners/admins with email addresses',
            },
          ],
          user_model_budgets: [
            {
              budget_id: 'budget_1',
              scope_key: 'budget:v1:user:user_1:upstream_model:gpt-5',
              user_id: 'user_1',
              model_id: null,
              upstream_model: 'gpt-5',
              budget: {
                cadence: 'daily',
                amount_usd: '10.0000',
                amount_usd_10000: 100_000,
                hard_limit: true,
                timezone: 'UTC',
              },
              current_window_spend_usd_10000: 0,
              alert_email_ready: true,
              alert_recipient_summary: 'jane@example.com',
            },
          ],
        },
      },
      alerts: {
        data: {
          items: [],
          page: 1,
          page_size: 10,
          total: 0,
        },
      },
      models: {
        data: {
          items: [],
        },
      },
    })

    const { SpendControlsPage } = await import('@/routes/spend-controls')
    render(<SpendControlsPage />)

    expect(screen.getByText('Spend Controls')).toBeInTheDocument()
    expect(screen.getAllByText('Jane Admin').length).toBeGreaterThan(0)
    expect(screen.getByText('CI Indexer')).toBeInTheDocument()
    expect(screen.getByText('User Model Budgets')).toBeInTheDocument()
    expect(screen.getByText('Budget Alert History')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Configure' }).length).toBeGreaterThan(0)
  })
})
