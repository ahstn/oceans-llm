import { beforeEach, describe, expect, it, vi } from 'vitest'

import { listApiKeys, listModels } from '@/server/admin-preview-data.server'
import {
  deactivateUser,
  getRequestLogDetail,
  getSpendReport,
  listBudgetAlertHistory,
  listRequestLogs,
  listSpendBudgets,
  listTeams,
  listUsers,
  reactivateUser,
  removeTeamMember,
  resetUserOnboarding,
  transferTeamMember,
  updateUser,
} from '@/server/admin-data.server'

const GET = vi.fn()
const POST = vi.fn()
const PATCH = vi.fn()
const PUT = vi.fn()
const DELETE = vi.fn()

vi.mock('@/server/gateway-client.server', () => ({
  createGatewayApiClient: () => ({
    GET,
    POST,
    PATCH,
    PUT,
    DELETE,
  }),
  unwrapGatewayResponse: vi.fn((result: { data?: unknown; error?: { error?: { message?: string } }; response: { status: number } }) => {
    if (result.data !== undefined) {
      return result.data
    }

    throw new Error(result.error?.error?.message ?? `Gateway request failed with ${result.response.status}`)
  }),
}))

describe('server-side admin data wrappers', () => {
  beforeEach(() => {
    GET.mockReset()
    POST.mockReset()
    PATCH.mockReset()
    PUT.mockReset()
    DELETE.mockReset()

    GET.mockImplementation(async (path: string) => {
      if (path === '/api/v1/admin/identity/users') {
        return {
          data: {
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
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      if (path === '/api/v1/admin/identity/teams') {
        return {
          data: {
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
                  members: [],
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
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      if (path === '/api/v1/admin/spend/report') {
        return {
          data: {
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
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      if (path === '/api/v1/admin/spend/budgets') {
        return {
          data: {
            data: { users: [], teams: [] },
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      if (path === '/api/v1/admin/spend/budget-alerts') {
        return {
          data: {
            data: { items: [], page: 1, page_size: 25, total: 0 },
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      if (path === '/api/v1/admin/observability/request-logs') {
        return {
          data: {
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
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      if (path === '/api/v1/admin/observability/request-logs/{request_log_id}') {
        return {
          data: {
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
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      throw new Error(`Unexpected GET path: ${path}`)
    })

    PATCH.mockResolvedValue({
      data: { data: { status: 'ok' }, meta: { generated_at: '2026-03-10T11:32:00Z' } },
      response: { status: 200 },
    })
    POST.mockImplementation(async (path: string) => {
      if (path === '/api/v1/admin/identity/users/{user_id}/reset-onboarding') {
        return {
          data: {
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
            meta: { generated_at: '2026-03-10T11:32:00Z' },
          },
          response: { status: 200 },
        }
      }

      return {
        data: { data: { status: 'ok' }, meta: { generated_at: '2026-03-10T11:32:00Z' } },
        response: { status: 200 },
      }
    })
    DELETE.mockResolvedValue({
      data: { data: { status: 'ok' }, meta: { generated_at: '2026-03-10T11:32:00Z' } },
      response: { status: 200 },
    })
  })

  it('returns stable preview and live envelopes', async () => {
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
    expect(detail.data.log.request_log_id).toBe('reqlog_1')
    expect(detail.data.payload?.response_json).toEqual({ body: { output: 'pong' } })
  })

  it('wires identity mutations to documented gateway paths', async () => {
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

    expect(PATCH).toHaveBeenCalledWith('/api/v1/admin/identity/users/{user_id}', {
      params: { path: { user_id: 'user_1' } },
      body: { global_role: 'platform_admin' },
    })
  })

  it('builds explicit request log tag query params', async () => {
    await listRequestLogs({
      service: 'checkout',
      tag_key: 'feature',
      tag_value: 'guest_checkout',
    })

    expect(GET).toHaveBeenCalledWith('/api/v1/admin/observability/request-logs', {
      params: {
        query: {
          service: 'checkout',
          tag_key: 'feature',
          tag_value: 'guest_checkout',
        },
      },
    })
  })
})
