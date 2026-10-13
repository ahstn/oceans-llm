import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import {
  amountInputValue,
  increaseExpiresAt,
  increasePresets,
  validateIncreaseAmount,
} from '@/routes/spend-controls/-budget-increase-model'

import {
  INHERITED_BUDGET_WARNING,
  INVALID_BUDGET_AMOUNT_MESSAGE,
  normalizeBudgetAmount,
} from '@/routes/spend-controls/-budget-model'
import { USER_PAGE_SIZE } from '@/routes/spend-controls/-user-list'
import { SpendControlsPage } from '@/routes/spend-controls'
import type { AuthSessionView, SpendBudgetUserView, SpendBudgetsView } from '@/types/api'
import { platformAdminSession, regularUserSession } from '@/test/auth-session'

const { routeMock, saveBudgetMock, increaseBudgetMock, toastErrorMock } = vi.hoisted(() => ({
  routeMock: { useLoaderData: vi.fn(), useRouteContext: vi.fn() },
  saveBudgetMock: vi.fn(),
  increaseBudgetMock: vi.fn(),
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
  increaseBudget: (...args: unknown[]) => increaseBudgetMock(...args),
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
    active_increase: null,
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

const unbudgeted = user(3, {
  name: 'No Budget',
  // The API reports zero spend for unbudgeted users, so they are never "quiet" and always show.
  current_window_spend_usd_10000: 0,
})

const budgets: SpendBudgetsView = {
  users: [jane, lowSpender, unbudgeted],
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

function renderPage(
  data: SpendBudgetsView = budgets,
  session: AuthSessionView = platformAdminSession(),
) {
  const isPlatformAdmin = session.user.global_role === 'platform_admin'
  routeMock.useLoaderData.mockReturnValue({
    budgets: { data },
    alerts: isPlatformAdmin ? { data: { items: [], page: 1, page_size: 10, total: 0 } } : null,
    models,
  })
  routeMock.useRouteContext.mockReturnValue({ session })
  render(
    <TooltipProvider>
      <SpendControlsPage />
    </TooltipProvider>,
  )
}

const monthlyHundred = {
  cadence: 'monthly',
  amount_usd: '100.0000',
  amount_usd_10000: 1_000_000,
  hard_limit: true,
  timezone: 'UTC',
}

/** $90 of $100: inside the 15% increase window. */
function nearLimitUser(index: number, overrides: Partial<SpendBudgetUserView> = {}) {
  return user(index, {
    name: `Near Limit ${index}`,
    team_id: 'team-1',
    team_name: 'Research',
    budget: monthlyHundred,
    budget_source: { kind: 'manual', key: null },
    current_window_spend_usd_10000: 900_000,
    ...overrides,
  })
}

function teamAdminSession(): AuthSessionView {
  return {
    ...regularUserSession(['spend_controls']),
    team_id: 'team-1',
    team_role: 'admin',
  }
}

function increaseButton(rowName: string) {
  const row = screen.getByRole('row', { name: new RegExp(rowName) })
  return within(row).getByRole('button', { name: 'Increase budget' })
}

function usersOnly(users: SpendBudgetUserView[]): SpendBudgetsView {
  return { users, service_accounts: [], user_model_budgets: [] }
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
    routeMock.useRouteContext.mockReset()
    saveBudgetMock.mockReset()
    increaseBudgetMock.mockReset()
    toastErrorMock.mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('sorts users by spend and hides quiet users by default', async () => {
    renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Spend controls' })).toBeInTheDocument()
    const rows = userRows()
    expect(rows[0]).toContain('Jane Admin')
    expect(rows[1]).toContain('No Budget')
    expect(rows).toHaveLength(2)
    expect(screen.getByText(/1 hidden by filters/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /Filters/ }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Hide quiet users' }))
    await waitFor(() => expect(userRows()).toHaveLength(3))
    expect(userRows()[1]).toContain('Low Spender')
  })

  it('summarises users and service accounts together', async () => {
    renderPage()

    // Jane $25 + Low Spender $5 + CI Indexer; unbudgeted users add nothing.
    const serviceSpend = budgets.service_accounts[0].current_window_spend_usd_10000 / 10_000
    expect(screen.getByText('Spend against budgets').nextElementSibling).toHaveTextContent(
      new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(
        30 + serviceSpend,
      ),
    )
    expect(screen.getByText('Budgeted owners').nextElementSibling).toHaveTextContent('3 / 4')
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

    fireEvent.click(screen.getAllByRole('button', { name: 'Configure' })[0])
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

    fireEvent.click(screen.getAllByRole('button', { name: 'Configure' })[0])
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

describe('budget increases', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    routeMock.useLoaderData.mockReset()
    routeMock.useRouteContext.mockReset()
    increaseBudgetMock.mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('shows the button on every row but enables it only within 15% of the limit', () => {
    const atThreshold = nearLimitUser(4, {
      name: 'At Threshold',
      current_window_spend_usd_10000: 850_000,
    })
    const overBudget = nearLimitUser(5, {
      name: 'Over Budget',
      current_window_spend_usd_10000: 1_200_000,
    })
    const underThreshold = nearLimitUser(6, {
      name: 'Under Threshold',
      current_window_spend_usd_10000: 840_000,
    })
    renderPage(usersOnly([atThreshold, overBudget, underThreshold, unbudgeted]))

    expect(increaseButton('At Threshold')).toBeEnabled()
    expect(increaseButton('Over Budget')).toBeEnabled()
    expect(increaseButton('Under Threshold')).toBeDisabled()
    expect(increaseButton('No Budget')).toBeDisabled()
  })

  it('lets team admins increase only their own team members, read-only otherwise', () => {
    const ownTeam = nearLimitUser(4, { name: 'Own Team' })
    const otherTeam = nearLimitUser(5, {
      name: 'Other Team',
      team_id: 'team-2',
      team_name: 'Applied',
    })
    renderPage(usersOnly([ownTeam, otherTeam]), teamAdminSession())

    expect(increaseButton('Own Team')).toBeEnabled()
    expect(increaseButton('Other Team')).toBeDisabled()
    expect(screen.queryByRole('button', { name: /Configure/ })).not.toBeInTheDocument()
    expect(screen.queryByText('Alert history')).not.toBeInTheDocument()
  })

  it('keeps the button disabled for team members', () => {
    renderPage(usersOnly([nearLimitUser(4, { name: 'Own Team' })]), regularUserSession())

    expect(increaseButton('Own Team')).toBeDisabled()
  })

  it('prefills presets, caps the amount at 3x, and submits the chosen period', async () => {
    increaseBudgetMock.mockResolvedValue({ data: {} })
    renderPage(usersOnly([nearLimitUser(4, { name: 'Own Team' })]))

    fireEvent.click(increaseButton('Own Team'))
    const dialog = await screen.findByRole('dialog')
    const presets = within(within(dialog).getByRole('group', { name: 'Prefill amount' }))
      .getAllByRole('button')
      .map((button) => button.textContent)
    expect(presets).toEqual(['$50.00', '$100.00', '$150.00', '$200.00'])
    expect(within(dialog).getByRole('radio', { name: 'End of month' })).toBeChecked()

    const amount = within(dialog).getByLabelText('Increase amount (USD)')
    fireEvent.change(amount, { target: { value: '300.01' } })
    expect(within(dialog).getByText(/cannot exceed \$300\.00/)).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Increase budget' }))
    expect(increaseBudgetMock).not.toHaveBeenCalled()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Prefill 150% of the budget' }))
    expect(amount).toHaveValue(150)
    fireEvent.click(within(dialog).getByRole('radio', { name: '7 days' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Increase budget' }))

    await waitFor(() =>
      expect(increaseBudgetMock).toHaveBeenCalledWith({
        data: { user_id: 'user_4', amount_usd: '150.0000', period: 'seven_days' },
      }),
    )
  })

  it('counts an active increase toward the limit and labels it', () => {
    renderPage(
      usersOnly([
        nearLimitUser(4, {
          name: 'Increased',
          active_increase: {
            budget_increase_id: 'increase_1',
            amount_usd: '50.0000',
            amount_usd_10000: 500_000,
            starts_at: '2026-10-10T00:00:00Z',
            expires_at: '2026-11-01T00:00:00Z',
          },
        }),
      ]),
    )

    const row = screen.getByRole('row', { name: /Increased/ })
    expect(within(row).getByText('+$50.00 until Oct 31')).toBeInTheDocument()
    expect(within(row).getByText('of $150.00')).toBeInTheDocument()
    // $90 of $150 is 60%: no longer inside the increase window.
    expect(increaseButton('Increased')).toBeDisabled()
  })
})

describe('budget increase model', () => {
  it('derives presets and input values from the base budget', () => {
    expect(increasePresets(1_500_000).map((preset) => preset.amount10000)).toEqual([
      750_000, 1_500_000, 2_250_000, 3_000_000,
    ])
    expect(amountInputValue(750_000)).toBe('75')
    expect(amountInputValue(1_125_000)).toBe('112.5')
    expect(amountInputValue(1)).toBe('0.0001')
  })

  it('accepts up to three times the base budget', () => {
    expect(validateIncreaseAmount('450', 1_500_000)).toEqual({
      ok: true,
      amountUsd: '450.0000',
      amount10000: 4_500_000,
    })
    expect(validateIncreaseAmount('450.0001', 1_500_000).ok).toBe(false)
    expect(validateIncreaseAmount('0', 1_500_000).ok).toBe(false)
    expect(validateIncreaseAmount('-5', 1_500_000).ok).toBe(false)
  })

  it('expires at the next UTC month boundary or seven days out', () => {
    const now = new Date('2026-12-15T18:30:00Z')
    expect(increaseExpiresAt('end_of_month', now).toISOString()).toBe('2027-01-01T00:00:00.000Z')
    expect(increaseExpiresAt('seven_days', now).toISOString()).toBe('2026-12-22T18:30:00.000Z')
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
