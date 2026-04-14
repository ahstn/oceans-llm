import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ApiKeysPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
}

const createGatewayApiKeyMock = vi.fn()
const revokeGatewayApiKeyMock = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => routerMock,
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  createGatewayApiKey: (...args: unknown[]) => createGatewayApiKeyMock(...args),
  getApiKeys: vi.fn(),
  revokeGatewayApiKey: (...args: unknown[]) => revokeGatewayApiKeyMock(...args),
}))

const basePayload: ApiKeysPayload = {
  items: [
    {
      id: 'api_key_1',
      name: 'Production Gateway',
      prefix: 'gwk_prod',
      status: 'active',
      owner_kind: 'team',
      owner_id: 'team_1',
      owner_name: 'Core Platform',
      owner_email: null,
      owner_team_key: 'core-platform',
      model_keys: ['fast'],
      created_at: '2026-03-14T12:00:00Z',
      last_used_at: '2026-03-18T09:15:00Z',
      revoked_at: null,
    },
  ],
  users: [
    {
      id: 'user_1',
      name: 'Jane Admin',
      email: 'jane@example.com',
    },
  ],
  teams: [
    {
      id: 'team_1',
      name: 'Core Platform',
      key: 'core-platform',
    },
  ],
  models: [
    {
      id: 'model_1',
      key: 'fast',
      description: 'Fast tier',
      tags: ['fast'],
    },
  ],
}

describe('ApiKeysPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })
    routerMock.invalidate.mockClear()
    createGatewayApiKeyMock.mockReset()
    revokeGatewayApiKeyMock.mockReset()
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn(async () => {}),
      },
    })
  })

  it('keeps create submission disabled until required fields are populated', async () => {
    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Create API key' })[0])

    expect(screen.getAllByRole('button', { name: 'Create API key' }).at(-1)).toBeDisabled()

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Production Web' } })

    expect(screen.getAllByRole('button', { name: 'Create API key' }).at(-1)).toBeDisabled()
  })

  it('shows the raw key once after a successful create flow', async () => {
    createGatewayApiKeyMock.mockResolvedValue({
      data: {
        api_key: {
          id: 'api_key_2',
          name: 'Production Web',
          prefix: 'gwk_prod_2',
          status: 'active',
          owner_kind: 'user',
          owner_id: 'user_1',
          owner_name: 'Jane Admin',
          owner_email: 'jane@example.com',
          owner_team_key: null,
          model_keys: ['fast'],
          created_at: '2026-03-20T09:00:00Z',
          last_used_at: null,
          revoked_at: null,
        },
        raw_key: 'gwk_prod_2.secret-value',
      },
    })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Create API key' })[0])
    const dialog = screen.getByRole('dialog', { name: 'Create API key' })
    fireEvent.change(within(dialog).getByLabelText('Name'), { target: { value: 'Production Web' } })
    fireEvent.click(screen.getByRole('combobox', { name: 'Owner user' }))
    fireEvent.click(screen.getByRole('option', { name: /Jane Admin/ }))
    fireEvent.click(within(dialog).getByRole('checkbox', { name: /fast/i }))

    const submitButton = within(dialog).getByRole('button', { name: 'Create API key' })
    await waitFor(() => expect(submitButton).toBeEnabled())
    fireEvent.click(submitButton)

    await waitFor(() => expect(createGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(screen.getByTestId('new-api-key-raw-key')).toHaveTextContent(
        'gwk_prod_2.secret-value',
      ),
    )
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })

  it('keeps create actions disabled until the mutation resolves', async () => {
    let resolveCreate: ((value: unknown) => void) | null = null
    createGatewayApiKeyMock.mockReturnValue(
      new Promise((resolve) => {
        resolveCreate = resolve
      }),
    )

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Create API key' })[0])
    const dialog = screen.getByRole('dialog', { name: 'Create API key' })
    fireEvent.change(within(dialog).getByLabelText('Name'), { target: { value: 'Production Web' } })
    fireEvent.click(screen.getByRole('combobox', { name: 'Owner user' }))
    fireEvent.click(screen.getByRole('option', { name: /Jane Admin/ }))
    fireEvent.click(within(dialog).getByRole('checkbox', { name: /fast/i }))

    const submitButton = within(dialog).getByRole('button', { name: 'Create API key' })
    await waitFor(() => expect(submitButton).toBeEnabled())
    fireEvent.click(submitButton)

    await waitFor(() => expect(createGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    expect(within(dialog).getByRole('button', { name: 'Creating...' })).toBeDisabled()

    resolveCreate?.({
      data: {
        api_key: {
          id: 'api_key_2',
          name: 'Production Web',
          prefix: 'gwk_prod_2',
          status: 'active',
          owner_kind: 'user',
          owner_id: 'user_1',
          owner_name: 'Jane Admin',
          owner_email: 'jane@example.com',
          owner_team_key: null,
          model_keys: ['fast'],
          created_at: '2026-03-20T09:00:00Z',
          last_used_at: null,
          revoked_at: null,
        },
        raw_key: 'gwk_prod_2.secret-value',
      },
    })

    await waitFor(() =>
      expect(screen.getByTestId('new-api-key-raw-key')).toHaveTextContent(
        'gwk_prod_2.secret-value',
      ),
    )
  })

  it('confirms revocation before calling the revoke mutation', async () => {
    revokeGatewayApiKeyMock.mockResolvedValue({
      data: {
        api_key: {
          ...basePayload.items[0],
          status: 'revoked',
          revoked_at: '2026-03-19T10:00:00Z',
        },
      },
    })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Revoke' })[0])

    expect(
      screen.getByText(/Existing callers will stop authenticating immediately/),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Revoke key' }))

    await waitFor(() => expect(revokeGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })
})
