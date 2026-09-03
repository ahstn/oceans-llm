import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  INVALID_BUDGET_AMOUNT_MESSAGE,
  normalizeBudgetAmount,
} from '@/routes/spend-controls/-utils'
import type { SpendBudgetsView } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
}

const saveBudgetMock = vi.fn()
const toastErrorMock = vi.fn()

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

const budgets: SpendBudgetsView = {
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
      budget_source: { kind: 'manual', key: null },
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
  ],
}

const models = [
  {
    id: 'fast',
    model_id: 'model_fast',
    resolved_model_key: 'fast',
    alias_of: null,
    tags: [],
    status: 'healthy',
    pricing_varies_by_route: false,
    client_configurations: [],
    allowlist: null,
  },
]

async function renderPage() {
  routeMock.useLoaderData.mockReturnValue({
    budgets: { data: budgets },
    alerts: { data: { items: [], page: 1, page_size: 10, total: 0 } },
    models: { data: { items: models } },
  })
  const { SpendControlsPage } = await import('@/routes/spend-controls')
  render(<SpendControlsPage />)
}

function userModelForm() {
  return screen.getByRole('button', { name: 'Add' }).closest('form')!
}

const INHERITED_WARNING =
  'This budget is inherited from configuration. Saving converts it to a manual budget that config reloads will not change.'

describe('SpendControlsPage', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    if (!Element.prototype.hasPointerCapture) {
      Element.prototype.hasPointerCapture = () => false
    }
    if (!Element.prototype.releasePointerCapture) {
      Element.prototype.releasePointerCapture = () => {}
    }
    routeMock.useLoaderData.mockReset()
    saveBudgetMock.mockReset()
    toastErrorMock.mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders user, service account, and user model budget management tables', async () => {
    await renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Spend controls' })).toBeInTheDocument()
    expect(screen.getAllByText('Jane Admin').length).toBeGreaterThan(0)
    expect(screen.getByText('CI Indexer')).toBeInTheDocument()
    expect(screen.getByText('User Model Budgets')).toBeInTheDocument()
    expect(screen.getByText('Budget Alert History')).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Configure' }).length).toBeGreaterThan(0)
  })

  it('labels each budget row with its source', async () => {
    await renderPage()

    expect(screen.getByText('Manual')).toBeInTheDocument()
    expect(screen.getByText('Config (service account)')).toBeInTheDocument()
    expect(screen.getByText('Config model default')).toBeInTheDocument()
  })

  it('warns only when editing a config-sourced budget', async () => {
    await renderPage()
    const [userConfigure, serviceAccountConfigure] = screen.getAllByRole('button', {
      name: 'Configure',
    })

    fireEvent.click(userConfigure)
    const manualDialog = screen.getByRole('dialog', { name: 'Configure Budget' })
    expect(within(manualDialog).getByText(/for Jane Admin\./)).toBeInTheDocument()
    expect(within(manualDialog).queryByText(INHERITED_WARNING)).not.toBeInTheDocument()
    fireEvent.click(within(manualDialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())

    fireEvent.click(serviceAccountConfigure)
    const inheritedDialog = screen.getByRole('dialog', { name: 'Configure Budget' })
    expect(within(inheritedDialog).getByText(/for CI Indexer\./)).toBeInTheDocument()
    expect(within(inheritedDialog).getByText(INHERITED_WARNING)).toBeInTheDocument()
  })

  it('exposes a timezone field on the add user model budget form', async () => {
    await renderPage()

    expect(screen.getByRole('textbox', { name: 'Timezone' })).toHaveValue('UTC')
  })

  it('sends a normalised 4-decimal amount and rejects non-positive amounts', async () => {
    saveBudgetMock.mockResolvedValue({})
    await renderPage()

    fireEvent.click(screen.getAllByRole('button', { name: 'Configure' })[0])
    const dialog = screen.getByRole('dialog', { name: 'Configure Budget' })
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
    await renderPage()
    const form = userModelForm()

    expect(within(form).getByRole('combobox', { name: 'User' })).toHaveTextContent('Jane Admin')
    expect(within(form).getByRole('combobox', { name: 'Model' })).toHaveTextContent('fast')
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
    await renderPage()
    const form = userModelForm()

    fireEvent.click(within(form).getByRole('combobox', { name: 'Scope type' }))
    fireEvent.click(screen.getByRole('option', { name: 'Upstream model' }))
    fireEvent.change(within(form).getByLabelText('Upstream model'), {
      target: { value: ' openai/gpt-5 ' },
    })
    fireEvent.change(within(form).getByLabelText('Amount (USD)'), { target: { value: '12.5' } })
    fireEvent.change(within(form).getByLabelText('Timezone'), {
      target: { value: 'Europe/London' },
    })
    fireEvent.click(within(form).getByRole('checkbox', { name: 'Hard' }))
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
    await renderPage()
    const form = userModelForm()

    fireEvent.click(within(form).getByRole('combobox', { name: 'Scope type' }))
    fireEvent.click(screen.getByRole('option', { name: 'Upstream model' }))
    fireEvent.change(within(form).getByLabelText('Amount (USD)'), { target: { value: '5' } })
    fireEvent.submit(form)

    expect(saveBudgetMock).not.toHaveBeenCalled()
    expect(toastErrorMock).toHaveBeenCalledTimes(1)
  })
})

describe('normalizeBudgetAmount', () => {
  it('pads to four decimals without rounding or losing precision', () => {
    expect(normalizeBudgetAmount('5')).toBe('5.0000')
    expect(normalizeBudgetAmount(' 12.5 ')).toBe('12.5000')
    expect(normalizeBudgetAmount('0.0001')).toBe('0.0001')
    expect(normalizeBudgetAmount('007.25')).toBe('7.2500')
    expect(normalizeBudgetAmount('10000000000000.0001')).toBe('10000000000000.0001')
  })

  it('rejects non-positive, over-precise, and non-numeric input', () => {
    expect(normalizeBudgetAmount('0')).toBeNull()
    expect(normalizeBudgetAmount('0.0000')).toBeNull()
    expect(normalizeBudgetAmount('0.00004')).toBeNull()
    expect(normalizeBudgetAmount('1.00005')).toBeNull()
    expect(normalizeBudgetAmount('-1')).toBeNull()
    expect(normalizeBudgetAmount('1e3')).toBeNull()
    expect(normalizeBudgetAmount('.5')).toBeNull()
    expect(normalizeBudgetAmount('')).toBeNull()
    expect(normalizeBudgetAmount('abc')).toBeNull()
  })
})
