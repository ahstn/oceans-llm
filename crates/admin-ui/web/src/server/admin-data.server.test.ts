import { describe, expect, it, vi } from 'vitest'

import {
  deactivateUser,
  getRequestLogDetail,
  listApiKeys,
  listModels,
  listRequestLogs,
  getSpendReport,
  listBudgetAlertHistory,
  listSpendBudgets,
  listTeams,
  listUsers,
  reactivateUser,
  removeTeamMember,
  resetUserOnboarding,
  transferTeamMember,
  updateUser,
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

    if (path === '/api/v1/admin/identity/users/user_1') {
      return { data: { status: 'ok' } }
    }

    if (path === '/api/v1/admin/identity/users/user_1/deactivate') {
      return { data: { status: 'ok' } }
    }

    if (path === '/api/v1/admin/identity/users/user_1/reactivate') {
      return { data: { status: 'ok' } }
    }

    if (path === '/api/v1/admin/identity/users/user_1/reset-onboarding') {
      return {
        data: {
          kind: 'password_invite',
          user: {
            id: 'user_1',
            name: 'Jane Admin',
            email: 'jane@example.com',
            auth_mode: 'password',
            global_role: 'platform_admin',
            team_id: 'team_1',
            team_name: 'Core Platform',
            team_role: 'owner',
            status: 'invited',
            onboarding: null,
          },
          invite_url: 'http://localhost:8080/admin/invite/test',
          expires_at: '2026-03-14T12:00:00Z',
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

    if (path === '/api/v1/admin/identity/teams/team_1/members/user_1') {
      return { data: { status: 'ok' } }
    }

    if (path === '/api/v1/admin/identity/teams/team_1/members/user_1/transfer') {
      return { data: { status: 'ok' } }
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

    if (path === '/api/v1/admin/spend/budget-alerts') {
      return {
        data: {
          items: [],
          page: 1,
          page_size: 25,
          total: 0,
        },
      }
    }

    if (path.startsWith('/api/v1/admin/observability/request-logs?') || path === '/api/v1/admin/observability/request-logs') {
      return {
        data: {
          items: [
            {
              request_log_id: 'reqlog_1',
              request_id: 'req_1',
              api_key_id: 'api_key_1',
              user_id: 'user_1',
              team_id: 'team_1',
              model_key: 'gpt-4.1-mini',
              resolved_model_key: 'gpt-4.1-mini',
              provider_key: 'openai',
              status_code: 200,
              latency_ms: 482,
              prompt_tokens: 400,
              completion_tokens: 942,
              total_tokens: 1342,
              error_code: null,
              has_payload: true,
              request_payload_truncated: false,
              response_payload_truncated: false,
              request_tags: {
                service: 'checkout',
                component: 'pricing_api',
                env: 'prod',
                bespoke: [{ key: 'feature', value: 'guest_checkout' }],
              },
              metadata: {
                stream: false,
                fallback_used: false,
                attempt_count: 1,
              },
              occurred_at: '2026-03-10T11:32:00Z',
            },
          ],
          page: 1,
          page_size: 1,
          total: 1,
        },
      }
    }

    if (path === '/api/v1/admin/observability/request-logs/reqlog_1') {
      return {
        data: {
          log: {
            request_log_id: 'reqlog_1',
            request_id: 'req_1',
            api_key_id: 'api_key_1',
            user_id: 'user_1',
            team_id: 'team_1',
            model_key: 'gpt-4.1-mini',
            resolved_model_key: 'gpt-4.1-mini',
            provider_key: 'openai',
            status_code: 200,
            latency_ms: 482,
            prompt_tokens: 400,
            completion_tokens: 942,
            total_tokens: 1342,
            error_code: null,
            has_payload: true,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: {
              service: 'checkout',
              component: 'pricing_api',
              env: 'prod',
              bespoke: [{ key: 'feature', value: 'guest_checkout' }],
            },
            metadata: {
              stream: false,
              fallback_used: false,
              attempt_count: 1,
            },
            occurred_at: '2026-03-10T11:32:00Z',
          },
          payload: {
            request_json: { body: { prompt: 'ping' } },
            response_json: { body: { output: 'pong' } },
          },
        },
      }
    }

    if (path === '/api/v1/admin/observability/request-logs/missing') {
      throw new Error('request log not found')
    }

    throw new Error(`Unexpected path: ${path}`)
  }),
}))

describe('server-side mock repositories', () => {
  it('returns stable API envelopes for phase-1 views', async () => {
    const [apiKeys, models, spendReport, spendBudgets, budgetAlerts, logs, teams, users] =
      await Promise.all([
      listApiKeys(),
      listModels(),
      getSpendReport(),
      listSpendBudgets(),
      listBudgetAlertHistory(),
      listRequestLogs(),
      listTeams(),
      listUsers(),
      ])

    expect(apiKeys.data.items.length).toBeGreaterThan(0)
    expect(models.data.length).toBeGreaterThan(0)
    expect(spendReport.data.window_days).toBeGreaterThan(0)
    expect(spendBudgets.data.users.length).toBe(0)
    expect(budgetAlerts.data.items.length).toBe(0)
    expect(logs.data.items.length).toBeGreaterThan(0)
    expect(teams.data.teams.length).toBeGreaterThan(0)
    expect(teams.data.users.length).toBeGreaterThan(0)
    expect(users.data.users.length).toBeGreaterThan(0)
    expect(users.data.teams.length).toBeGreaterThan(0)
  })

  it('treats request log detail as a strict fetch', async () => {
    const detail = await getRequestLogDetail('reqlog_1')
    expect(detail.data.log.requestLogId).toBe('reqlog_1')
    expect(detail.data.payload?.responseJson).toEqual({ body: { output: 'pong' } })

    await expect(getRequestLogDetail('missing')).rejects.toThrow('request log not found')
  })

  it('wires identity mutations to the documented gateway paths', async () => {
    await expect(
      updateUser('user_1', { global_role: 'platform_admin' }),
    ).resolves.toMatchObject({ data: { status: 'ok' } })
    await expect(deactivateUser('user_1')).resolves.toMatchObject({ data: { status: 'ok' } })
    await expect(reactivateUser('user_1')).resolves.toMatchObject({ data: { status: 'ok' } })
    await expect(removeTeamMember('team_1', 'user_1')).resolves.toMatchObject({
      data: { status: 'ok' },
    })
    await expect(
      transferTeamMember('team_1', 'user_1', {
        destination_team_id: 'team_2',
        destination_role: 'member',
      }),
    ).resolves.toMatchObject({ data: { status: 'ok' } })

    const reset = await resetUserOnboarding('user_1')
    expect(reset.data.kind).toBe('password_invite')
    expect(reset.data.invite_url).toContain('/admin/invite/')
  })

  it('builds explicit request log tag query params', async () => {
    await listRequestLogs({
      service: 'checkout',
      tagKey: 'feature',
      tagValue: 'guest_checkout',
    })

    const { fetchGatewayJson } = await import('@/server/gateway-client.server')
    expect(fetchGatewayJson).toHaveBeenCalledWith(
      '/api/v1/admin/observability/request-logs?service=checkout&tag_key=feature&tag_value=guest_checkout',
    )
  })
})
