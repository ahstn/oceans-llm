import { describe, expect, it, vi } from 'vitest'

import {
  listApiKeys,
  listModels,
  listRequestLogs,
  getSpendReport,
  listSpendBudgets,
  listTeams,
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

    if (path === '/api/v1/admin/identity/teams') {
      return {
        data: {
          teams: [
            {
              id: 'team_1',
              name: 'Core Platform',
              key: 'core-platform',
              status: 'active',
              member_count: 2,
              admins: [
                {
                  id: 'user_1',
                  name: 'Jane Admin',
                  email: 'jane@example.com',
                  status: 'active',
                },
              ],
            },
          ],
          users: [
            {
              id: 'user_1',
              name: 'Jane Admin',
              email: 'jane@example.com',
              status: 'active',
              team_id: 'team_1',
              team_name: 'Core Platform',
              team_role: 'admin',
            },
          ],
          oidc_providers: [{ id: 'oidc_1', key: 'okta-main', label: 'okta-main' }],
        },
      }
    }

    if (path.startsWith('/api/v1/admin/spend/report')) {
      return {
        data: {
          window_days: 7,
          owner_kind: 'all',
          window_start: '2026-03-01T00:00:00Z',
          window_end: '2026-03-08T00:00:00Z',
          totals: {
            priced_cost_usd_10000: 123_450,
            priced_request_count: 42,
            unpriced_request_count: 2,
            usage_missing_request_count: 1,
          },
          daily: [],
          owners: [],
          models: [],
        },
      }
    }

    if (path === '/api/v1/admin/spend/budgets') {
      return {
        data: {
          users: [],
          teams: [],
        },
      }
    }

    throw new Error(`Unexpected path: ${path}`)
  }),
}))

describe('server-side mock repositories', () => {
  it('returns stable API envelopes for phase-1 views', async () => {
    const [apiKeys, models, spendReport, spendBudgets, logs, teams, users] = await Promise.all([
      listApiKeys(),
      listModels(),
      getSpendReport(),
      listSpendBudgets(),
      listRequestLogs(),
      listTeams(),
      listUsers(),
    ])

    expect(apiKeys.data.items.length).toBeGreaterThan(0)
    expect(models.data.length).toBeGreaterThan(0)
    expect(spendReport.data.window_days).toBeGreaterThan(0)
    expect(spendBudgets.data.users.length).toBe(0)
    expect(logs.data.items.length).toBeGreaterThan(100)
    expect(teams.data.teams.length).toBeGreaterThan(0)
    expect(teams.data.users.length).toBeGreaterThan(0)
    expect(users.data.users.length).toBeGreaterThan(0)
    expect(users.data.teams.length).toBeGreaterThan(0)
  })
})
