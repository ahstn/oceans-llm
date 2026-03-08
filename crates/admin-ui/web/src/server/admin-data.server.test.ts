import { describe, expect, it, vi } from 'vitest'

import {
  listApiKeys,
  listModels,
  listRequestLogs,
  listTeams,
  listUsageCosts,
  listUsers,
} from '@/server/admin-data.server'

vi.mock('@/server/gateway-client.server', () => ({
  fetchGatewayJson: vi.fn(async (path: string) => {
    if (path === '/api/v1/admin/identity/users') {
      return {
        data: {
          users: [
            {
              id: 'user_1',
              name: 'Jane Admin',
              email: 'jane@example.com',
              auth_mode: 'password',
              global_role: 'platform_admin',
              team_id: 'team_1',
              team_name: 'Core Platform',
              team_role: 'owner',
              status: 'invited',
              onboarding: {
                kind: 'password_invite',
                invite_url: 'http://localhost:8080/admin/invite/test',
                expires_at: '2026-03-14T12:00:00Z',
                can_resend: true,
              },
            },
          ],
          teams: [{ id: 'team_1', name: 'Core Platform' }],
          oidc_providers: [{ id: 'oidc_1', key: 'okta-main', label: 'okta-main' }],
        },
      }
    }

    throw new Error(`Unexpected path: ${path}`)
  }),
}))

describe('server-side mock repositories', () => {
  it('returns stable API envelopes for phase-1 views', async () => {
    const [apiKeys, models, costs, logs, teams, users] = await Promise.all([
      listApiKeys(),
      listModels(),
      listUsageCosts(),
      listRequestLogs(),
      listTeams(),
      listUsers(),
    ])

    expect(apiKeys.data.items.length).toBeGreaterThan(0)
    expect(models.data.length).toBeGreaterThan(0)
    expect(costs.data.length).toBeGreaterThan(0)
    expect(logs.data.items.length).toBeGreaterThan(100)
    expect(teams.data.length).toBeGreaterThan(0)
    expect(users.data.users.length).toBeGreaterThan(0)
    expect(users.data.teams.length).toBeGreaterThan(0)
  })
})
