import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ApiKeysPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useSearch: vi.fn(),
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
const revealGatewayApiKeySecretMock = vi.fn()
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
  revealGatewayApiKeySecret: (...args: unknown[]) => revealGatewayApiKeySecretMock(...args),
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
      owner_service_account_key: null,
      owner_service_account_team_id: null,
      owner_service_account_team_key: null,
      model_grant_mode: 'explicit',
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
  service_accounts: [
    {
      id: 'service_account_1',
      name: 'Deploy Bot',
      key: 'deploy-bot',
      team_id: 'team_1',
      team_key: 'core-platform',
      team_name: 'Core Platform',
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
    cleanup()
    vi.stubGlobal('ResizeObserver', ResizeObserverMock)
    routeMock.useLoaderData.mockReset()
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })
    routeMock.useRouteContext.mockReset()
    routeMock.useRouteContext.mockReturnValue({
      session: {
        must_change_password: false,
        permissions: {
          group: 'platform_admins',
          pages: ['api_keys'],
          actions: ['create_api_key', 'update_api_key', 'revoke_api_key', 'reveal_api_key'],
          default_page: 'api_keys',
        },
        user: {
          id: 'admin_1',
          name: 'Admin User',
          email: 'admin@example.com',
          global_role: 'platform_admin',
        },
      },
    })
    routeMock.useSearch.mockReset()
    routeMock.useSearch.mockReturnValue({ api_key_id: undefined })
    routerMock.invalidate.mockClear()
    createGatewayApiKeyMock.mockReset()
    revealGatewayApiKeySecretMock.mockReset()
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
          owner_service_account_key: null,
          owner_service_account_team_id: null,
          owner_service_account_team_key: null,
          model_grant_mode: 'all',
          model_keys: [],
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
        owner_service_account_id: null,
        model_grant_mode: 'all',
        model_keys: [],
      },
    })
    await waitFor(() =>
      expect(screen.getByTestId('new-api-key-raw-key')).toHaveTextContent(
        'gwk_prod_2.secret-value',
      ),
    )
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })

  it('creates service-account-owned API keys with the selected service account owner', async () => {
    createGatewayApiKeyMock.mockResolvedValue({
      data: {
        api_key: {
          id: 'api_key_2',
          name: 'Deploy Worker',
          prefix: 'gwk_deploy_live_987654321',
          status: 'active',
          owner_kind: 'service_account',
          owner_id: 'service_account_1',
          owner_name: 'Deploy Bot',
          owner_email: null,
          owner_team_key: 'core-platform',
          owner_service_account_key: 'deploy-bot',
          owner_service_account_team_id: 'team_1',
          owner_service_account_team_key: 'core-platform',
          model_grant_mode: 'explicit',
          model_keys: ['fast'],
          created_at: '2026-03-20T09:00:00Z',
          last_used_at: null,
          revoked_at: null,
        },
        raw_key: 'gwk_deploy_2.secret-value',
      },
    })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Create API key' })[0])
    const dialog = screen.getByRole('dialog', { name: 'Create API key' })
    fireEvent.change(within(dialog).getByLabelText('Name'), { target: { value: 'Deploy Worker' } })
    fireEvent.click(within(dialog).getByRole('combobox', { name: 'Owner type' }))
    fireEvent.click(screen.getByRole('option', { name: 'Service account' }))
    fireEvent.click(within(dialog).getByRole('combobox', { name: 'Owner service account' }))
    fireEvent.click(screen.getByRole('option', { name: /Deploy Bot/ }))
    await toggleModelSelection(dialog, 'fast')

    const submitButton = within(dialog).getByRole('button', { name: 'Create API key' })
    await waitFor(() => expect(submitButton).toBeEnabled())
    fireEvent.click(submitButton)

    await waitFor(() => expect(createGatewayApiKeyMock).toHaveBeenCalledTimes(1))
    expect(createGatewayApiKeyMock).toHaveBeenCalledWith({
      data: {
        name: 'Deploy Worker',
        owner_kind: 'service_account',
        owner_user_id: null,
        owner_team_id: 'team_1',
        owner_service_account_id: 'service_account_1',
        model_grant_mode: 'explicit',
        model_keys: ['fast'],
      },
    })
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
          owner_service_account_key: null,
          owner_service_account_team_id: null,
          owner_service_account_team_key: null,
          model_grant_mode: 'all',
          model_keys: [],
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

  it('opens the targeted manage dialog from the api_key_id search param', async () => {
    routeMock.useSearch.mockReturnValue({ api_key_id: 'api_key_1' })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    const dialog = await screen.findByRole('dialog', { name: 'Manage API key' })
    expect(within(dialog).getByText('Production Gateway')).toBeInTheDocument()
    expect(within(dialog).getByText('gwk_prod_liv****')).toBeInTheDocument()
  })

  it('does not reopen an already dismissed api_key_id deeplink after items refresh', async () => {
    routeMock.useSearch.mockReturnValue({ api_key_id: 'api_key_1' })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    const { rerender } = render(<ApiKeysPage />)

    const dialog = await screen.findByRole('dialog', { name: 'Manage API key' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())

    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        items: basePayload.items.map((item) => ({ ...item })),
      },
    })
    rerender(<ApiKeysPage />)

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('opens the manage dialog and updates model access when the selection changes', async () => {
    updateGatewayApiKeyMock.mockResolvedValue({
      data: {
        api_key: {
          ...basePayload.items[0],
          model_grant_mode: 'explicit',
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

    const summary = within(dialog).getByTestId('manage-api-key-summary')
    expect(summary).toHaveClass('border-y')
    expect(summary).not.toHaveClass('rounded-lg')
    expect(summary).not.toHaveClass('bg-[color:var(--color-surface-muted)]')

    const metadata = within(dialog).getByTestId('manage-api-key-metadata')
    expect(metadata).toHaveClass('divide-y')
    expect(within(metadata).getByText('Owner').closest('div')).toHaveTextContent('Jane Admin')
    expect(within(metadata).getByText('Created').closest('div')).toHaveTextContent('2026-03-14')
    expect(within(metadata).getByText('Last used').closest('div')).toHaveTextContent(
      '2026-03-18 09:15',
    )

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
          model_grant_mode: 'explicit',
          model_keys: ['fast', 'reasoning'],
        },
      },
    })
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })

  it('reveals and copies retrievable service-account-owned API keys from the manage dialog', async () => {
    revealGatewayApiKeySecretMock.mockResolvedValue({
      data: { raw_key: 'gwk_service_account.secret-value' },
    })
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        items: [
          {
            ...basePayload.items[0],
            owner_kind: 'service_account',
            owner_id: 'service_account_1',
            owner_name: 'Deploy Bot',
            owner_email: null,
            owner_team_key: 'core-platform',
            owner_service_account_key: 'deploy-bot',
            owner_service_account_team_id: 'team_1',
            owner_service_account_team_key: 'core-platform',
          },
        ],
      },
    })

    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])

    const dialog = screen.getByRole('dialog', { name: 'Manage API key' })
    expect(within(dialog).getByText('Credential secret')).toBeInTheDocument()

    fireEvent.click(within(dialog).getByRole('button', { name: 'Reveal API key' }))

    await waitFor(() => expect(revealGatewayApiKeySecretMock).toHaveBeenCalledTimes(1))
    expect(revealGatewayApiKeySecretMock).toHaveBeenCalledWith({
      data: { apiKeyId: 'api_key_1' },
    })
    expect(await within(dialog).findByTestId('manage-api-key-raw-key')).toHaveTextContent(
      'gwk_service_account.secret-value',
    )

    fireEvent.click(within(dialog).getByRole('button', { name: 'Copy API key' }))

    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        'gwk_service_account.secret-value',
      ),
    )
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

  it('lets regular users create and manage their own keys', async () => {
    routeMock.useRouteContext.mockReturnValue({
      session: {
        must_change_password: false,
        permissions: {
          group: 'users',
          pages: ['api_keys'],
          actions: ['create_api_key', 'update_api_key', 'revoke_api_key'],
          default_page: 'api_keys',
        },
        user: {
          id: 'user_1',
          name: 'Jane User',
          email: 'jane@example.com',
          global_role: 'user',
        },
      },
    })
    const { ApiKeysPage } = await import('@/routes/api-keys')

    render(<ApiKeysPage />)

    expect(screen.getAllByText('gwk_prod_liv****').length).toBeGreaterThan(0)
    expect(
      screen.getByText('Create and manage API keys within your access scope.'),
    ).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Create API key' })).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Manage' }).length).toBeGreaterThan(0)

    fireEvent.click(screen.getByRole('button', { name: 'Create API key' }))
    const createDialog = screen.getByRole('dialog', { name: 'Create API key' })
    expect(within(createDialog).getByRole('combobox', { name: 'Owner user' })).toHaveTextContent(
      'Jane Admin',
    )
    expect(within(createDialog).queryByRole('option', { name: 'Service account' })).toBeNull()

    fireEvent.click(within(createDialog).getByRole('button', { name: 'Cancel' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])
    const manageDialog = screen.getByRole('dialog', { name: 'Manage API key' })
    expect(within(manageDialog).getByRole('button', { name: 'Save access' })).toBeInTheDocument()
    expect(within(manageDialog).getByRole('button', { name: 'Revoke key' })).toBeInTheDocument()
    expect(within(manageDialog).queryByRole('button', { name: 'Reveal API key' })).toBeNull()
  })
})

async function toggleModelSelection(dialog: HTMLElement, modelKey: string) {
  fireEvent.click(
    within(dialog).getByRole('button', { name: /Select models|models selected|fast|reasoning/i }),
  )
  fireEvent.click(await screen.findByRole('option', { name: new RegExp(modelKey, 'i') }))
}
