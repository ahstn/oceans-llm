import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { RateLimitPolicyView } from '@/types/api'
import {
  ANY,
  describeLimit,
  describeScope,
  emptyPolicyForm,
  policyInputFromForm,
} from '@/routes/rate-limits/-policy-model'

const invalidateMock = vi.fn()
const addRateLimitPolicyMock = vi.fn()
const saveRateLimitPolicyMock = vi.fn()
const removeRateLimitPolicyMock = vi.fn()
const toastErrorMock = vi.fn()

const configPolicy: RateLimitPolicyView = {
  id: null,
  name: 'default per key',
  source: 'config',
  operation: 'inference',
  scope_kind: 'api_key',
  scope_id: null,
  scope_label: null,
  model_key: null,
  mcp_server_id: null,
  mcp_server_key: null,
  metric: 'requests',
  period: 'minute',
  limit: 600,
  enabled: true,
  updated_at: null,
}

const adminPolicy: RateLimitPolicyView = {
  id: 'policy_1',
  name: 'ops team notes',
  source: 'admin',
  operation: 'mcp_tool_call',
  scope_kind: 'team',
  scope_id: 'team_1',
  scope_label: 'Ops',
  model_key: null,
  mcp_server_id: 'server_1',
  mcp_server_key: 'notes',
  metric: 'requests',
  period: 'month',
  limit: 5000,
  enabled: false,
  updated_at: '2026-10-15T10:00:00Z',
}

let loaderData: unknown

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock)

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => ({ useLoaderData: () => loaderData }),
  useRouter: () => ({ navigate: vi.fn(), invalidate: invalidateMock }),
}))

vi.mock('sonner', () => ({
  toast: { error: (...args: unknown[]) => toastErrorMock(...args), success: vi.fn() },
}))

vi.mock('@/server/admin-data.functions', () => ({
  addRateLimitPolicy: (...args: unknown[]) => addRateLimitPolicyMock(...args),
  saveRateLimitPolicy: (...args: unknown[]) => saveRateLimitPolicyMock(...args),
  removeRateLimitPolicy: (...args: unknown[]) => removeRateLimitPolicyMock(...args),
  getApiKeys: vi.fn(),
  getMcpServers: vi.fn(),
  getRateLimitPolicies: vi.fn(),
  getTeams: vi.fn(),
}))

const { RateLimitsPage } = await import('@/routes/rate-limits')

function renderPage(policies: RateLimitPolicyView[]) {
  loaderData = {
    policies,
    options: {
      apiKeys: [{ value: 'key_1', label: 'CI (gwk_ci)' }],
      users: [{ value: 'user_1', label: 'ada@example.test' }],
      teams: [{ value: 'team_1', label: 'Ops' }],
      models: [{ value: 'fast', label: 'fast' }],
      mcpServers: [{ value: 'server_1', label: 'notes' }],
    },
  }
  return render(<RateLimitsPage />)
}

beforeEach(() => {
  invalidateMock.mockResolvedValue(undefined)
  addRateLimitPolicyMock.mockResolvedValue({ data: adminPolicy })
  saveRateLimitPolicyMock.mockResolvedValue({ data: adminPolicy })
  removeRateLimitPolicyMock.mockResolvedValue({ data: adminPolicy })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('rate limit policy model', () => {
  it('describes limits and scopes for people', () => {
    expect(describeLimit(configPolicy)).toBe('600 requests / minute')
    expect(describeLimit({ ...configPolicy, metric: 'tokens', limit: 100000 })).toBe(
      '100,000 tokens / minute',
    )
    expect(describeScope(configPolicy)).toBe('Each API key')
    expect(describeScope(adminPolicy)).toBe('Team: Ops')
    expect(describeScope({ ...configPolicy, scope_kind: 'global' })).toBe('Whole gateway')
  })

  it('drops filters that do not apply to the operation', () => {
    const form = {
      ...emptyPolicyForm(),
      name: ' calls ',
      operation: 'mcp_tool_call' as const,
      scopeKind: 'global' as const,
      scopeId: 'user_1',
      modelKey: 'fast',
      mcpServerId: 'server_1',
      metric: 'tokens' as const,
      limit: '20',
    }
    expect(policyInputFromForm(form)).toEqual({
      name: 'calls',
      operation: 'mcp_tool_call',
      scope_kind: 'global',
      scope_id: null,
      model_key: null,
      mcp_server_id: 'server_1',
      metric: 'requests',
      period: 'minute',
      limit: 20,
      enabled: true,
    })
    expect(policyInputFromForm({ ...emptyPolicyForm(), name: 'x', modelKey: ANY })).toMatchObject({
      model_key: null,
      scope_id: null,
      mcp_server_id: null,
    })
  })
})

describe('RateLimitsPage', () => {
  it('lists config and admin policies, with config read-only', () => {
    renderPage([configPolicy, adminPolicy])

    const table = screen.getByRole('table')
    const rows = within(table).getAllByRole('row')
    expect(rows).toHaveLength(3)
    expect(within(rows[1]).getByText('default per key')).toBeTruthy()
    expect(within(rows[1]).getByText('Config')).toBeTruthy()
    expect(within(rows[1]).getByText('Edit in gateway.yaml')).toBeTruthy()
    expect(within(rows[2]).getByText('5,000 requests / month')).toBeTruthy()
    expect(within(rows[2]).getByText('MCP tool calls · notes')).toBeTruthy()
    expect(within(rows[2]).getByText('Team: Ops')).toBeTruthy()
    expect(within(rows[2]).getByText('Paused')).toBeTruthy()
    expect(within(rows[2]).getByRole('button', { name: 'Edit' })).toBeTruthy()
  })

  it('shows an empty state without policies', () => {
    renderPage([])
    expect(screen.getByText('No rate limits')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Add the first rate limit' }))
    expect(screen.getByRole('heading', { name: 'New rate limit' })).toBeTruthy()
  })

  it('creates a policy with the default per-key minute limit', async () => {
    renderPage([])
    fireEvent.click(screen.getByRole('button', { name: 'New rate limit' }))
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'per key' } })
    fireEvent.change(screen.getByLabelText('Limit'), { target: { value: '120' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create rate limit' }))

    await waitFor(() => expect(addRateLimitPolicyMock).toHaveBeenCalledTimes(1))
    expect(addRateLimitPolicyMock).toHaveBeenCalledWith({
      data: {
        name: 'per key',
        operation: 'inference',
        scope_kind: 'api_key',
        scope_id: null,
        model_key: null,
        mcp_server_id: null,
        metric: 'requests',
        period: 'minute',
        limit: 120,
        enabled: true,
      },
    })
    await waitFor(() => expect(invalidateMock).toHaveBeenCalled())
  })

  it('keeps the target and filters when an admin policy is edited', async () => {
    renderPage([adminPolicy])
    const table = screen.getByRole('table')
    fireEvent.click(within(table).getByRole('button', { name: 'Edit' }))
    fireEvent.change(screen.getByLabelText('Limit'), { target: { value: '7000' } })
    fireEvent.click(screen.getByLabelText('Enforced'))
    fireEvent.click(screen.getByRole('button', { name: 'Save rate limit' }))

    await waitFor(() => expect(saveRateLimitPolicyMock).toHaveBeenCalledTimes(1))
    expect(saveRateLimitPolicyMock).toHaveBeenCalledWith({
      data: {
        policyId: 'policy_1',
        input: {
          name: 'ops team notes',
          operation: 'mcp_tool_call',
          scope_kind: 'team',
          scope_id: 'team_1',
          model_key: null,
          mcp_server_id: 'server_1',
          metric: 'requests',
          period: 'month',
          limit: 7000,
          enabled: true,
        },
      },
    })
  })

  it('rejects a limit that is not a positive whole number', () => {
    renderPage([])
    fireEvent.click(screen.getByRole('button', { name: 'New rate limit' }))
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'bad' } })
    const limit = screen.getByLabelText('Limit')
    fireEvent.change(limit, { target: { value: '0' } })
    const message = screen.getByRole('alert')
    expect(message.textContent).toBe('Limit must be a whole number above 0.')
    expect(limit.getAttribute('aria-invalid')).toBe('true')
    expect(limit.getAttribute('aria-describedby')).toBe(message.id)
    expect(
      (screen.getByRole('button', { name: 'Create rate limit' }) as HTMLButtonElement).disabled,
    ).toBe(true)
  })

  it('explains a blank name on the name field', () => {
    renderPage([])
    fireEvent.click(screen.getByRole('button', { name: 'New rate limit' }))
    const name = screen.getByLabelText('Name')
    expect(screen.queryByRole('alert')).toBeNull()
    fireEvent.change(name, { target: { value: '   ' } })
    const message = screen.getByRole('alert')
    expect(message.textContent).toBe('Enter a name.')
    expect(name.getAttribute('aria-invalid')).toBe('true')
    expect(name.getAttribute('aria-describedby')).toBe(message.id)
    expect(
      (screen.getByRole('button', { name: 'Create rate limit' }) as HTMLButtonElement).disabled,
    ).toBe(true)
  })

  it('deletes an admin policy after confirmation', async () => {
    renderPage([adminPolicy])
    const table = screen.getByRole('table')
    fireEvent.click(within(table).getByRole('button', { name: 'Delete' }))
    fireEvent.click(screen.getByRole('button', { name: 'Delete rate limit' }))

    await waitFor(() =>
      expect(removeRateLimitPolicyMock).toHaveBeenCalledWith({ data: { policyId: 'policy_1' } }),
    )
  })
})
