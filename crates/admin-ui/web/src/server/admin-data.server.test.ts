import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  createApiKey,
  deactivateUser,
  getRequestLogDetail,
  getSpendReport,
  getUsageLeaderboard,
  listApiKeys,
  listModels,
  listBudgetAlertHistory,
  listRequestLogs,
  listSpendBudgets,
  listTeams,
  listUsers,
  logoutCurrentSession,
  reactivateUser,
  removeTeamMember,
  revokeApiKey,
  resetUserOnboarding,
  transferTeamMember,
  updateApiKey,
  updateUser,
} from '@/server/admin-data.server'

const GET = vi.fn()
const POST = vi.fn()
const PATCH = vi.fn()
const PUT = vi.fn()
const DELETE = vi.fn()
const fetchGatewayJson = vi.fn()

vi.mock('@/server/gateway-client.server', () => ({
  createGatewayApiClient: () => ({
    GET,
    POST,
    PATCH,
    PUT,
    DELETE,
  }),
  fetchGatewayJson: (...args: unknown[]) => fetchGatewayJson(...args),
  unwrapGatewayResponse: vi.fn(
    (result: {
      data?: unknown
      error?: { error?: { message?: string } }
      response: { status: number }
    }) => {
      if (result.data !== undefined) {
        return result.data
      }

      throw new Error(
        result.error?.error?.message ?? `Gateway request failed with ${result.response.status}`,
      )
    },
  ),
}))

describe('server-side admin data wrappers', () => {
  beforeEach(() => {
    GET.mockReset()
    POST.mockReset()
    PATCH.mockReset()
    PUT.mockReset()
    DELETE.mockReset()
    fetchGatewayJson.mockReset()

    fetchGatewayJson.mockImplementation(async (path: string, init?: { method?: string }) => {
      if (path === '/api/v1/admin/api-keys' && (!init?.method || init.method === 'GET')) {
        return {
          data: {
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
                model_keys: ['fast', 'reasoning'],
                created_at: '2026-03-14T12:00:00Z',
                last_used_at: '2026-03-18T09:15:00Z',
                revoked_at: null,
              },
            ],
            users: [{ id: 'user_1', name: 'Jane Admin', email: 'jane@example.com' }],
            teams: [{ id: 'team_1', name: 'Core Platform', key: 'core-platform' }],
            models: [
              { id: 'model_1', key: 'fast', description: 'Fast tier', tags: ['fast'] },
              {
                id: 'model_2',
                key: 'reasoning',
                description: 'Reasoning tier',
                tags: ['reasoning'],
              },
            ],
          },
          meta: { generated_at: '2026-03-10T11:32:00Z' },
        }
      }

      if (path === '/api/v1/admin/api-keys' && init?.method === 'POST') {
        return {
          data: {
            api_key: {
              id: 'api_key_2',
              name: 'Production Gateway',
              prefix: 'gwk_prod_2',
              status: 'active',
              owner_kind: 'team',
              owner_id: 'team_1',
              owner_name: 'Core Platform',
              owner_email: null,
              owner_team_key: 'core-platform',
              model_keys: ['fast'],
              created_at: '2026-03-20T09:00:00Z',
              last_used_at: null,
              revoked_at: null,
            },
            raw_key: 'gwk_prod_2.secret-value',
          },
          meta: { generated_at: '2026-03-10T11:32:00Z' },
        }
      }

      if (path === '/api/v1/admin/models') {
        return {
          data: [
            {
              id: 'claude-sonnet',
              resolved_model_key: 'claude-sonnet',
              alias_of: null,
              description: 'Claude on Vertex',
              tags: ['reasoning'],
              status: 'healthy',
              provider_key: 'vertex-claude',
              provider_label: 'Google Vertex AI',
              provider_icon_key: 'vertexai',
              upstream_model: 'anthropic/claude-sonnet-4-6',
              model_icon_key: 'claude',
              input_cost_per_million_tokens_usd_10000: 30_000,
              output_cost_per_million_tokens_usd_10000: 150_000,
              context_window_tokens: 200_000,
              input_window_tokens: null,
              output_window_tokens: 64_000,
              supports_streaming: true,
              supports_vision: false,
              supports_tool_calling: false,
              supports_structured_output: true,
              supports_attachments: true,
            },
          ],
          meta: { generated_at: '2026-03-10T11:32:00Z' },
        }
      }

      if (path === '/api/v1/admin/api-keys/api_key_1/revoke') {
        return {
          data: {
            api_key: {
              id: 'api_key_1',
              name: 'Production Gateway',
              prefix: 'gwk_prod',
              status: 'revoked',
              owner_kind: 'team',
              owner_id: 'team_1',
              owner_name: 'Core Platform',
              owner_email: null,
              owner_team_key: 'core-platform',
              model_keys: ['fast', 'reasoning'],
              created_at: '2026-03-14T12:00:00Z',
              last_used_at: '2026-03-18T09:15:00Z',
              revoked_at: '2026-03-19T10:00:00Z',
            },
          },
          meta: { generated_at: '2026-03-10T11:32:00Z' },
        }
      }

      throw new Error(`Unexpected path: ${path}`)
    })

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

      if (path === '/api/v1/admin/models') {
        return {
          data: {
            data: {
              items: [
                {
                  id: 'claude-sonnet',
                  resolved_model_key: 'claude-sonnet',
                  alias_of: null,
                  description: 'Claude on Vertex',
                  tags: ['reasoning'],
                  status: 'healthy',
                  provider_key: 'vertex-claude',
                  provider_label: 'Google Vertex AI',
                  provider_icon_key: 'vertexai',
                  upstream_model: 'anthropic/claude-sonnet-4-6',
                  model_icon_key: 'claude',
                  input_cost_per_million_tokens_usd_10000: 30_000,
                  output_cost_per_million_tokens_usd_10000: 150_000,
                  context_window_tokens: 200_000,
                  input_window_tokens: null,
                  output_window_tokens: 64_000,
                  supports_streaming: true,
                  supports_vision: false,
                  supports_tool_calling: false,
                  supports_structured_output: true,
                  supports_attachments: true,
                },
              ],
              page: 1,
              page_size: 30,
              total: 1,
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

      if (path === '/api/v1/admin/observability/leaderboard') {
        return {
          data: {
            data: {
              range: '7d',
              window_start: '2026-03-01T00:00:00Z',
              window_end: '2026-03-08T00:00:00Z',
              bucket_hours: 12,
              chart_users: [
                {
                  user_id: 'user_1',
                  user_name: 'Jane Admin',
                  total_spend_usd_10000: 123_450,
                },
              ],
              series: [
                {
                  bucket_start: '2026-03-01T00:00:00Z',
                  values: [
                    {
                      user_id: 'user_1',
                      spend_usd_10000: 123_450,
                    },
                  ],
                },
              ],
              leaders: [
                {
                  user_id: 'user_1',
                  user_name: 'Jane Admin',
                  total_spend_usd_10000: 123_450,
                  most_used_model: 'fast',
                  total_requests: 42,
                },
              ],
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
                  },
                  payload_policy: {
                    capture_mode: 'redacted_payloads',
                    request_max_bytes: 65536,
                    response_max_bytes: 65536,
                    stream_max_events: 128,
                    version: 'builtin:v1',
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
                },
                payload_policy: {
                  capture_mode: 'redacted_payloads',
                  request_max_bytes: 65536,
                  response_max_bytes: 65536,
                  stream_max_events: 128,
                  version: 'builtin:v1',
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

    PATCH.mockImplementation(async (path: string) => {
      if (path === '/api/v1/admin/api-keys/{api_key_id}') {
        return {
          data: {
            data: {
              api_key: {
                id: 'api_key_1',
                name: 'Production Gateway',
                prefix: 'gwk_prod',
                status: 'active',
                owner_kind: 'team',
                owner_id: 'team_1',
                owner_name: 'Core Platform',
                owner_email: null,
                owner_team_key: 'core-platform',
                model_keys: ['reasoning'],
                created_at: '2026-03-14T12:00:00Z',
                last_used_at: '2026-03-18T09:15:00Z',
                revoked_at: null,
              },
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
    POST.mockImplementation(async (path: string) => {
      if (path === '/api/v1/auth/logout') {
        return {
          data: { data: { status: 'ok' }, meta: { generated_at: '2026-03-10T11:32:00Z' } },
          response: { status: 200 },
        }
      }

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
    const [
      apiKeys,
      models,
      spendReport,
      leaderboard,
      spendBudgets,
      budgetAlerts,
      logs,
      teams,
      users,
    ] = await Promise.all([
      listApiKeys(),
      listModels(),
      getSpendReport(),
      getUsageLeaderboard({ range: '7d' }),
      listSpendBudgets(),
      listBudgetAlertHistory(),
      listRequestLogs(),
      listTeams(),
      listUsers(),
    ])

    expect(apiKeys.data.items.length).toBeGreaterThan(0)
    expect(apiKeys.data.items[0].owner_kind).toBe('team')
    expect(apiKeys.data.models.map((model) => model.key)).toContain('fast')
    expect(models.data.items.length).toBeGreaterThan(0)
    expect(spendReport.data.window_days).toBeGreaterThan(0)
    expect(leaderboard.data.chart_users.length).toBe(1)
    expect(leaderboard.data.leaders[0].most_used_model).toBe('fast')
    expect(spendBudgets.data.users.length).toBe(0)
    expect(budgetAlerts.data.items.length).toBe(0)
    expect(logs.data.items.length).toBeGreaterThan(0)
    expect(teams.data.teams.length).toBeGreaterThan(0)
    expect(teams.data.users.length).toBeGreaterThan(0)
    expect(users.data.users.length).toBeGreaterThan(0)
    expect(users.data.teams.length).toBeGreaterThan(0)
  })

  it('wires api key mutations to the documented gateway paths', async () => {
    await expect(
      createApiKey({
        name: 'Production Gateway',
        owner_kind: 'team',
        owner_user_id: null,
        owner_team_id: 'team_1',
        model_keys: ['fast'],
      }),
    ).resolves.toMatchObject({
      data: {
        api_key: {
          id: 'api_key_2',
          status: 'active',
        },
        raw_key: 'gwk_prod_2.secret-value',
      },
    })

    await expect(updateApiKey('api_key_1', { model_keys: ['reasoning'] })).resolves.toMatchObject({
      data: {
        api_key: {
          id: 'api_key_1',
          model_keys: ['reasoning'],
        },
      },
    })

    await expect(revokeApiKey('api_key_1')).resolves.toMatchObject({
      data: {
        api_key: {
          id: 'api_key_1',
          status: 'revoked',
        },
      },
    })

    expect(PATCH).toHaveBeenCalledWith('/api/v1/admin/api-keys/{api_key_id}', {
      params: { path: { api_key_id: 'api_key_1' } },
      body: { model_keys: ['reasoning'] },
    })
  })

  it('treats request log detail as a strict fetch', async () => {
    const detail = await getRequestLogDetail('reqlog_1')
    expect(detail.data.log.request_log_id).toBe('reqlog_1')
    expect(detail.data.payload?.response_json).toEqual({ body: { output: 'pong' } })
  })

  it('wires leaderboard fetches to the documented gateway path and query params', async () => {
    const leaderboard = await getUsageLeaderboard({ range: '31d' })

    expect(leaderboard.data.range).toBe('7d')
    expect(GET).toHaveBeenCalledWith('/api/v1/admin/observability/leaderboard', {
      params: {
        query: {
          range: '31d',
        },
      },
    })
  })

  it('wires identity mutations to documented gateway paths', async () => {
    await expect(updateUser('user_1', { global_role: 'platform_admin' })).resolves.toMatchObject({
      data: { status: 'ok' },
    })
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

  it('wires logout to the documented gateway path', async () => {
    await expect(logoutCurrentSession()).resolves.toMatchObject({ data: { status: 'ok' } })

    expect(POST).toHaveBeenCalledWith('/api/v1/auth/logout')
  })
})
