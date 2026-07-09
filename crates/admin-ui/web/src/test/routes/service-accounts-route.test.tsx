import type * as React from 'react'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ApiKeysPayload, ServiceAccountsPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  Link: ({
    to,
    search,
    children,
    ...props
  }: {
    to: string
    search?: Record<string, string | undefined>
    children: React.ReactNode
  }) => {
    const query = search
      ? `?${new URLSearchParams(
          Object.entries(search).filter((entry): entry is [string, string] => Boolean(entry[1])),
        ).toString()}`
      : ''

    return (
      <a href={`${to}${query}`} {...props}>
        {children}
      </a>
    )
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  getApiKeys: vi.fn(),
  getServiceAccounts: vi.fn(),
}))

const serviceAccountsPayload: ServiceAccountsPayload = {
  service_accounts: [
    {
      id: 'service_account_1',
      name: 'Deploy Bot',
      key: 'deploy-bot',
      status: 'active',
      tags: [{ key: 'workload', value: 'deploy' }],
      team_id: 'team_1',
      team_key: 'core-platform',
      team_name: 'Core Platform',
    },
    {
      id: 'service_account_2',
      name: 'Nightly Rollup',
      key: 'nightly-rollup',
      status: 'disabled',
      tags: [],
      team_id: 'team_2',
      team_key: 'analytics',
      team_name: 'Analytics',
    },
  ],
  teams: [
    { id: 'team_1', name: 'Core Platform' },
    { id: 'team_2', name: 'Analytics' },
  ],
}

const apiKeysPayload: ApiKeysPayload = {
  items: [
    {
      id: 'api_key_1',
      name: 'Deploy Worker Key',
      prefix: 'gwk_deploy_live_123456789',
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
      created_at: '2026-03-14T12:00:00Z',
      last_used_at: null,
      revoked_at: null,
    },
    {
      id: 'api_key_user_1',
      name: 'Human Operator Key',
      prefix: 'gwk_user_live_123456789',
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
      created_at: '2026-03-14T12:00:00Z',
      last_used_at: null,
      revoked_at: null,
    },
  ],
  users: [],
  service_accounts: [],
  models: [],
}

describe('ServiceAccountsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useLoaderData.mockReturnValue({
      serviceAccounts: serviceAccountsPayload.service_accounts,
      apiKeys: apiKeysPayload.items,
    })
  })

  it('renders service accounts with teams, status, and attached API-key names', async () => {
    const { ServiceAccountsPage } = await import('@/routes/identity/service-accounts')

    render(<ServiceAccountsPage />)

    expect(screen.getAllByText('Deploy Bot').length).toBeGreaterThan(0)
    expect(screen.getAllByText('deploy-bot').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Core Platform').length).toBeGreaterThan(0)
    expect(screen.getAllByText('active').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Deploy Worker Key').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Nightly Rollup').length).toBeGreaterThan(0)
    expect(screen.getAllByText('No credential attached').length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: /create|add|disable|revoke|rename/i })).not.toBeInTheDocument()
  })

  it('links teams to the identity Teams page', async () => {
    const { ServiceAccountsPage } = await import('@/routes/identity/service-accounts')

    render(<ServiceAccountsPage />)

    const teamLinks = screen.getAllByRole('link', { name: 'Open Core Platform in Teams' })
    expect(teamLinks[0]).toHaveAttribute('href', '/identity/teams')
  })

  it('links attached credentials to the API Keys deeplink target', async () => {
    const { ServiceAccountsPage } = await import('@/routes/identity/service-accounts')

    render(<ServiceAccountsPage />)

    const apiKeyLinks = screen.getAllByRole('link', { name: 'Open API key Deploy Worker Key' })
    expect(apiKeyLinks[0]).toHaveAttribute('href', '/api-keys?api_key_id=api_key_1')
  })

  it('explains the scoped empty state without mutation controls', async () => {
    routeMock.useLoaderData.mockReturnValue({ serviceAccounts: [], apiKeys: [] })

    const { ServiceAccountsPage } = await import('@/routes/identity/service-accounts')

    render(<ServiceAccountsPage />)

    expect(screen.getByText('No service accounts visible')).toBeInTheDocument()
    expect(screen.getByText(/No service accounts are visible for the current scope/)).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })
})
