import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ApiKeysPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
}

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
}

const createGatewayApiKeyMock = vi.fn()
const revokeGatewayApiKeyMock = vi.fn()
const updateGatewayApiKeyMock = vi.fn()

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
  updateGatewayApiKey: (...args: unknown[]) => updateGatewayApiKeyMock(...args),
}))

const basePayload: ApiKeysPayload = {
  items: [
    {
      id: 'api_key_1',
      name: 'Production Gateway',
      prefix: 'gwk_prod_live_123456789',
      status: 'active',
      owner_kind: 'user',
      owner_id: 'user_1',
      owner_name: 'Jane Admin',
      owner_email: 'jane@example.com',
      owner_team_key: null,
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
    {
      id: 'model_2',
      key: 'reasoning',
      description: 'Reasoning tier',
      tags: ['reasoning'],
    },
  ],
}

describe('ApiKeysPage', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    routeMock.useLoaderData.mockReset()
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })
    routerMock.invalidate.mockClear()
    createGatewayApiKeyMock.mockReset()
    revokeGatewayApiKeyMock.mockReset()
    updateGatewayApiKeyMock.mockReset()
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn(async () => {}),
      },
    })
  })

  it('renders masked prefixes and normalized owner and timestamp metadata', async () => {
    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    expect(screen.getAllByText('gwk_prod_liv****').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Jane Admin').length).toBeGreaterThan(0)
    expect(screen.queryByText('jane@example.com')).not.toBeInTheDocument()
    expect(screen.getAllByText('2026-03-14').length).toBeGreaterThan(0)
    expect(screen.getAllByText('2026-03-18 09:15').length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: 'Copy prefix' })).not.toBeInTheDocument()
  })

  it('keeps create submission disabled until required fields are populated', async () => {
    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Create API key' })[0])

    expect(screen.getAllByRole('button', { name: 'Create API key' }).at(-1)).toBeDisabled()

    const dialog = screen.getByRole('dialog', { name: 'Create API key' })
    fireEvent.change(within(dialog).getByLabelText('Name'), { target: { value: 'Production Web' } })

    expect(within(dialog).getByRole('button', { name: 'Create API key' })).toBeDisabled()
  })

  it('shows the raw key once after a successful create flow', async () => {
    createGatewayApiKeyMock.mockResolvedValue({
      data: {
        api_key: {
          id: 'api_key_2',
          name: 'Production Web',
          prefix: 'gwk_prod_live_987654321',
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
    await toggleModelSelection(dialog, 'fast')

    const submitButton = within(dialog).getByRole('button', { name: 'Create API key' })
    await waitFor(() => expect(submitButton).toBeEnabled())
    fireEvent.click(submitButton)

    await waitFor(() => expect(createGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    expect(createGatewayApiKeyMock).toHaveBeenCalledWith({
      data: {
        name: 'Production Web',
        owner_kind: 'user',
        owner_user_id: 'user_1',
        owner_team_id: null,
        model_keys: ['fast'],
      },
    })
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
    await toggleModelSelection(dialog, 'fast')

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
          prefix: 'gwk_prod_live_987654321',
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

  it('opens the manage dialog and updates model access when the selection changes', async () => {
    updateGatewayApiKeyMock.mockResolvedValue({
      data: {
        api_key: {
          ...basePayload.items[0],
          model_keys: ['fast', 'reasoning'],
        },
      },
    })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])

    const dialog = screen.getByRole('dialog', { name: 'Manage API key' })
    expect(within(dialog).getByText('gwk_prod_liv****')).toBeInTheDocument()
    expect(within(dialog).getByText('Jane Admin')).toBeInTheDocument()
    expect(within(dialog).queryByText('jane@example.com')).not.toBeInTheDocument()
    expect(within(dialog).getByText('2026-03-14')).toBeInTheDocument()
    expect(within(dialog).getByText('2026-03-18 09:15')).toBeInTheDocument()

    const saveButton = within(dialog).getByRole('button', { name: 'Save access' })
    expect(saveButton).toBeDisabled()

    await toggleModelSelection(dialog, 'reasoning')
    await waitFor(() => expect(saveButton).toBeEnabled())
    fireEvent.click(saveButton)

    await waitFor(() => expect(updateGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    expect(updateGatewayApiKeyMock).toHaveBeenCalledWith({
      data: {
        apiKeyId: 'api_key_1',
        input: {
          model_keys: ['fast', 'reasoning'],
        },
      },
    })
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })

  it('revokes from the manage dialog lifecycle section', async () => {
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

    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])

    const dialog = screen.getByRole('dialog', { name: 'Manage API key' })
    expect(
      within(dialog).getByText(/Revocation takes effect immediately and cannot be undone/),
    ).toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Revoke key' }))

    await waitFor(() => expect(revokeGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    expect(revokeGatewayApiKeyMock).toHaveBeenCalledWith({
      data: { apiKeyId: 'api_key_1' },
    })
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })
})

async function toggleModelSelection(dialog: HTMLElement, modelKey: string) {
  fireEvent.click(within(dialog).getByRole('button', { name: /Select models|models selected|fast|reasoning/i }))
  fireEvent.click(await screen.findByRole('option', { name: new RegExp(modelKey, 'i') }))
}
