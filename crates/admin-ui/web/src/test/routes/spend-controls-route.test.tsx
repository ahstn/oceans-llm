import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  INHERITED_BUDGET_WARNING,
  INVALID_BUDGET_AMOUNT_MESSAGE,
  normalizeBudgetAmount,
} from '@/routes/spend-controls/-budget-model'
import { USER_PAGE_SIZE } from '@/routes/spend-controls/-user-list'
import { SpendControlsPage } from '@/routes/spend-controls'
import type { SpendBudgetUserView, SpendBudgetsView } from '@/types/api'

const { routeMock, saveBudgetMock, toastErrorMock } = vi.hoisted(() => ({
  routeMock: { useLoaderData: vi.fn() },
  saveBudgetMock: vi.fn(),
  toastErrorMock: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({
    invalidate: vi.fn(async () => {}),
  }),
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  getBudgetAlertHistory: vi.fn(),
  getModels: vi.fn(),
  getSpendBudgets: vi.fn(),
  removeBudget: vi.fn(),
  saveBudget: (...args: unknown[]) => saveBudgetMock(...args),
}))

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function user(index: number, overrides: Partial<SpendBudgetUserView> = {}): SpendBudgetUserView {
  return {
    user_id: `user_${index}`,
    name: `User ${String(index).padStart(2, '0')}`,
    email: `user${index}@example.com`,
    team_id: null,
    team_name: null,
    budget: null,
    budget_source: null,
    current_window_spend_usd_10000: 0,
    alert_email_ready: true,
    alert_recipient_summary: `user${index}@example.com`,
    ...overrides,
  }
}

const jane = user(1, {
  name: 'Jane Admin',
  email: 'jane@example.com',
  budget: {
    cadence: 'daily',
    amount_usd: '100.0000',
    amount_usd_10000: 1_000_000,
    hard_limit: true,
    timezone: 'UTC',
  },
  budget_source: { kind: 'manual', key: null },
  // 25% of budget: above the 20% "quiet" cutoff, so visible by default.
  current_window_spend_usd_10000: 250_000,
})

const lowSpender = user(2, {
  name: 'Low Spender',
  budget: {
    cadence: 'daily',
    amount_usd: '100.0000',
    amount_usd_10000: 1_000_000,
    hard_limit: true,
    timezone: 'UTC',
  },
  budget_source: { kind: 'config_user_default', key: null },
  // 5% of budget: hidden by the default filter.
  current_window_spend_usd_10000: 50_000,
})

const bigSpender = user(3, {
  name: 'Big Spender',
  // No budget but real spend: sorts first and is never "quiet".
  current_window_spend_usd_10000: 900_000,
})

const budgets: SpendBudgetsView = {
  users: [jane, lowSpender, bigSpender],
  service_accounts: [
    {
      service_account_id: 'service_account_1',
      service_account_name: 'CI Indexer',
      service_account_key: 'ci-indexer',
      team_id: 'team_1',
      team_name: 'Core Platform',
      team_key: 'core-platform',
      budget: {
        cadence: 'monthly',
        amount_usd: '250.0000',
        amount_usd_10000: 2_500_000,
        hard_limit: true,
        timezone: 'UTC',
      },
      budget_source: { kind: 'config_service_account', key: 'ci-indexer' },
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
      budget_source: { kind: 'config_user_model_default', key: null },
      current_window_spend_usd_10000: 0,
      alert_email_ready: true,
      alert_recipient_summary: 'jane@example.com',
    },
    {
      budget_id: 'budget_2',
      scope_key: 'budget:v1:user:user_1:model:model_fast',
      user_id: 'user_1',
      model_id: 'model_fast',
      upstream_model: null,
      budget: {
        cadence: 'daily',
        amount_usd: '5.0000',
        amount_usd_10000: 50_000,
        hard_limit: true,
        timezone: 'UTC',
      },
      budget_source: { kind: 'manual', key: null },
      current_window_spend_usd_10000: 0,
      alert_email_ready: true,
      alert_recipient_summary: 'jane@example.com',
    },
  ],
}

const models = [
  {
    id: 'fast',
    model_id: 'model_fast',
    resolved_model_key: 'fast',
    alias_of: null,
    tags: [],
    status: 'healthy' as const,
    pricing_varies_by_route: false,
    client_configurations: [],
    allowlist: null,
  },
]

function renderPage(data: SpendBudgetsView = budgets) {
  routeMock.useLoaderData.mockReturnValue({
    budgets: { data },
    alerts: { data: { items: [], page: 1, page_size: 10, total: 0 } },
    models: { data: { items: models } },
  })
  render(<SpendControlsPage />)
}

// Radix tabs activate on pointer down, not click.
function openTab(name: RegExp) {
  fireEvent.mouseDown(screen.getByRole('tab', { name }), { button: 0 })
}

function userRows() {
  const table = screen.getByRole('table')
  return within(table)
    .getAllByRole('row')
    .slice(1)
    .map((row) => within(row).getAllByRole('cell')[0].textContent ?? '')
}

function modelBudgetForm() {
  return screen.getByRole('button', { name: 'Add model budget' }).closest('form')!
}

describe('SpendControlsPage', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    if (!Element.prototype.hasPointerCapture) {
      Element.prototype.hasPointerCapture = () => false
    }
    if (!Element.prototype.releasePointerCapture) {
      Element.prototype.releasePointerCapture = () => {}
    }
    if (!Element.prototype.scrollIntoView) {
      Element.prototype.scrollIntoView = () => {}
    }
    routeMock.useLoaderData.mockReset()
    saveBudgetMock.mockReset()
    toastErrorMock.mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('sorts users by spend and hides quiet users by default', async () => {
    renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Spend controls' })).toBeInTheDocument()
    const rows = userRows()
    expect(rows[0]).toContain('Big Spender')
    expect(rows[1]).toContain('Jane Admin')
    expect(rows).toHaveLength(2)
    expect(screen.getByText(/1 hidden by filters/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /Filters/ }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Hide quiet users' }))
    await waitFor(() => expect(userRows()).toHaveLength(3))
    expect(userRows()[2]).toContain('Low Spender')
  })

  it('pages users fifteen at a time', async () => {
    const many = Array.from({ length: USER_PAGE_SIZE + 3 }, (_, index) =>
      user(index + 10, { current_window_spend_usd_10000: (index + 1) * 10_000 }),
    )
    renderPage({ ...budgets, users: many })

    expect(userRows()).toHaveLength(USER_PAGE_SIZE)
    expect(screen.getByText(`Showing 1–${USER_PAGE_SIZE} of ${many.length} users`)).toBeVisible()

    fireEvent.click(screen.getByRole('link', { name: 'Go to next page' }))
    expect(userRows()).toHaveLength(3)
    expect(screen.queryByRole('link', { name: 'Go to next page' })).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Go to previous page' })).toBeInTheDocument()
  })

  it('labels inherited budgets with their source', async () => {
    renderPage()

    expect(screen.queryByText('Manual')).not.toBeInTheDocument()

    openTab(/Service accounts/)
    expect(screen.getByText('CI Indexer')).toBeInTheDocument()
    expect(screen.getByText('Config (service account)')).toBeInTheDocument()

    openTab(/Model budgets/)
    expect(screen.getByText('Config model default')).toBeInTheDocument()
  })

  it('resolves model budget ids to model keys', async () => {
    renderPage()
    openTab(/Model budgets/)

    expect(screen.getByText('model:fast')).toBeInTheDocument()
    expect(screen.getByText('upstream:gpt-5')).toBeInTheDocument()
  })

  it('warns only when editing a config-sourced budget', async () => {
    renderPage()

    fireEvent.click(screen.getAllByRole('button', { name: 'Configure' })[1])
    const manualDialog = screen.getByRole('dialog', { name: 'Configure budget' })
    expect(within(manualDialog).getByText(/for Jane Admin\./)).toBeInTheDocument()
    expect(within(manualDialog).queryByText(INHERITED_BUDGET_WARNING)).not.toBeInTheDocument()
    fireEvent.click(within(manualDialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())

    openTab(/Service accounts/)
    fireEvent.click(screen.getByRole('button', { name: 'Configure' }))
    const inheritedDialog = screen.getByRole('dialog', { name: 'Configure budget' })
    expect(within(inheritedDialog).getByText(/for CI Indexer\./)).toBeInTheDocument()
    expect(within(inheritedDialog).getByText(INHERITED_BUDGET_WARNING)).toBeInTheDocument()
  })

  it('sends a normalised 4-decimal amount and rejects non-positive amounts', async () => {
    saveBudgetMock.mockResolvedValue({})
    renderPage()

    fireEvent.click(screen.getAllByRole('button', { name: 'Configure' })[1])
    const dialog = screen.getByRole('dialog', { name: 'Configure budget' })
    const amount = within(dialog).getByLabelText('Amount (USD)')

    fireEvent.change(amount, { target: { value: '0' } })
    fireEvent.submit(amount.closest('form')!)
    expect(toastErrorMock).toHaveBeenCalledWith(INVALID_BUDGET_AMOUNT_MESSAGE)
    expect(saveBudgetMock).not.toHaveBeenCalled()

    fireEvent.change(amount, { target: { value: '5' } })
    fireEvent.submit(amount.closest('form')!)
    await waitFor(() => expect(saveBudgetMock).toHaveBeenCalledTimes(1))
    expect(saveBudgetMock).toHaveBeenCalledWith({
      data: {
        scope: { kind: 'user', user_id: 'user_1' },
        cadence: 'daily',
        amount_usd: '5.0000',
        hard_limit: true,
        timezone: 'UTC',
      },
    })
  })

  it('creates a user model budget for a managed model', async () => {
    saveBudgetMock.mockResolvedValue({})
    renderPage()
    openTab(/Model budgets/)
    const form = modelBudgetForm()

    expect(within(form).getByRole('combobox', { name: 'User' })).toHaveTextContent('Jane Admin')
    expect(within(form).getByRole('combobox', { name: 'Model' })).toHaveTextContent('fast')
    expect(within(form).getByLabelText('Timezone')).toHaveValue('UTC')
    fireEvent.change(within(form).getByLabelText('Amount (USD)'), { target: { value: '40' } })
    fireEvent.click(within(form).getByRole('combobox', { name: 'Cadence' }))
    fireEvent.click(screen.getByRole('option', { name: 'Monthly' }))
    fireEvent.submit(form)

    await waitFor(() => expect(saveBudgetMock).toHaveBeenCalledTimes(1))
    expect(saveBudgetMock).toHaveBeenCalledWith({
      data: {
        scope: { kind: 'user_model', user_id: 'user_1', model_id: 'model_fast' },
        cadence: 'monthly',
        amount_usd: '40.0000',
        hard_limit: true,
        timezone: 'UTC',
      },
    })
  })

  it('creates a user model budget for an upstream model name', async () => {
    saveBudgetMock.mockResolvedValue({})
    renderPage()
    openTab(/Model budgets/)
    const form = modelBudgetForm()

    fireEvent.click(within(form).getByRole('combobox', { name: 'Scope' }))
    fireEvent.click(screen.getByRole('option', { name: 'Upstream model' }))
    fireEvent.change(within(form).getByLabelText('Upstream model'), {
      target: { value: ' openai/gpt-5 ' },
    })
    fireEvent.change(within(form).getByLabelText('Amount (USD)'), { target: { value: '12.5' } })
    fireEvent.change(within(form).getByLabelText('Timezone'), {
      target: { value: 'Europe/London' },
    })
    fireEvent.click(within(form).getByRole('switch', { name: 'Enforce hard limit' }))
    fireEvent.submit(form)

    await waitFor(() => expect(saveBudgetMock).toHaveBeenCalledTimes(1))
    expect(saveBudgetMock).toHaveBeenCalledWith({
      data: {
        scope: { kind: 'user_model', user_id: 'user_1', upstream_model: 'openai/gpt-5' },
        cadence: 'daily',
        amount_usd: '12.5000',
        hard_limit: false,
        timezone: 'Europe/London',
      },
    })
  })

  it('rejects a user model budget without a model selector', async () => {
    renderPage()
    openTab(/Model budgets/)
    const form = modelBudgetForm()

    fireEvent.click(within(form).getByRole('combobox', { name: 'Scope' }))
    fireEvent.click(screen.getByRole('option', { name: 'Upstream model' }))
    fireEvent.change(within(form).getByLabelText('Amount (USD)'), { target: { value: '5' } })
    fireEvent.submit(form)

    expect(saveBudgetMock).not.toHaveBeenCalled()
    expect(toastErrorMock).toHaveBeenCalledTimes(1)
  })
})

describe('normalizeBudgetAmount', () => {
  it('pads to four decimals without rounding', () => {
    expect(normalizeBudgetAmount('5')).toBe('5.0000')
    expect(normalizeBudgetAmount(' 12.5 ')).toBe('12.5000')
    expect(normalizeBudgetAmount('0.0001')).toBe('0.0001')
    expect(normalizeBudgetAmount('007.25')).toBe('7.2500')
  })

  it('rejects non-positive, malformed, and over-precise amounts', () => {
    expect(normalizeBudgetAmount('0')).toBeNull()
    expect(normalizeBudgetAmount('0.0000')).toBeNull()
    expect(normalizeBudgetAmount('-5')).toBeNull()
    expect(normalizeBudgetAmount('1.23456')).toBeNull()
    expect(normalizeBudgetAmount('abc')).toBeNull()
    expect(normalizeBudgetAmount('')).toBeNull()
  })
})
