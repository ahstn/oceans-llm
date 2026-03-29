import { expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv, stubAdminUrl } from './env'

const gatewayApiKey = process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value'

test('gateway exposes the seeded model and forwards chat completions to the stub upstream', async ({
  request,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')

  const modelsResponse = await request.get(`${root}/v1/models`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
    },
  })

  expect(modelsResponse.ok()).toBe(true)
  expect(await modelsResponse.json()).toEqual({
    object: 'list',
    data: [
      {
        id: 'fast',
        object: 'model',
        created: 0,
        owned_by: 'gateway',
      },
    ],
  })

  const clearResponse = await request.delete(stubAdminUrl('/__admin/requests'))
  expect(clearResponse.ok()).toBe(true)

  const completionResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: 'ping' }],
    },
  })

  expect(completionResponse.status()).toBe(200)
  expect(completionResponse.headers()['x-request-id']).toBeTruthy()
  expect(await completionResponse.json()).toEqual({
    id: 'chatcmpl-e2e-1',
    object: 'chat.completion',
    created: 1_741_510_000,
    model: 'fast',
    choices: [
      {
        index: 0,
        message: {
          role: 'assistant',
          content: 'pong',
        },
        finish_reason: 'stop',
      },
    ],
    usage: {
      prompt_tokens: 80_000,
      completion_tokens: 40_000,
      total_tokens: 120_000,
    },
  })

  const capturedResponse = await request.get(stubAdminUrl('/__admin/requests'))
  expect(capturedResponse.ok()).toBe(true)

  const capturedPayload = (await capturedResponse.json()) as {
    requests: Array<{
      method: string
      path: string
      headers: Record<string, string>
      body: {
        model: string
        messages: Array<{ role: string; content: string }>
      }
    }>
  }

  expect(capturedPayload.requests).toHaveLength(1)

  const [captured] = capturedPayload.requests
  expect(captured.method).toBe('POST')
  expect(captured.path).toBe('/v1/chat/completions')
  expect(captured.headers.authorization).toBe('Bearer upstream-e2e-token')
  expect(captured.body.model).toBe('gpt-4o-mini')
  expect(captured.body.messages).toEqual([
    expect.objectContaining({ role: 'user', content: 'ping' }),
  ])
})

test('admin spend report endpoint and usage costs page reflect live usage ledger data', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)

  const completionResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
      'idempotency-key': 'e2e-spend-report-live',
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: 'spend report probe' }],
    },
  })
  expect(completionResponse.status()).toBe(200)

  const reportResponse = await request.get(`${root}/api/v1/admin/spend/report?days=7&owner_kind=all`, {
    headers: {
      cookie: adminCookie,
    },
  })
  expect(reportResponse.status()).toBe(200)
  const reportBody = (await reportResponse.json()) as {
    data: {
      totals: {
        priced_cost_usd_10000: number
        priced_request_count: number
      }
      models: Array<{ model_key: string }>
    }
  }
  expect(reportBody.data.totals.priced_request_count).toBeGreaterThanOrEqual(1)
  expect(reportBody.data.totals.priced_cost_usd_10000).toBeGreaterThanOrEqual(0)
  expect(reportBody.data.models.some((model) => model.model_key === 'fast')).toBe(true)

  const pricedSpendLabel = new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(reportBody.data.totals.priced_cost_usd_10000 / 10_000)

  await page.goto('/admin/observability/usage-costs')
  await expect(page.getByRole('heading', { name: 'Usage Costs' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Owner Breakdown' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Model Breakdown' })).toBeVisible()
  await expect(page.getByText(pricedSpendLabel).first()).toBeVisible()
  await expect(page.getByText('fast').first()).toBeVisible()
})

test('team budget update triggers hard-limit enforcement for team-owned keys', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)

  const budgetsResponse = await request.get(`${root}/api/v1/admin/spend/budgets`, {
    headers: {
      cookie: adminCookie,
    },
  })
  expect(budgetsResponse.status()).toBe(200)
  const budgetsBody = (await budgetsResponse.json()) as {
    data: {
      teams: Array<{ team_id: string; team_key: string }>
    }
  }
  expect(budgetsBody.data.teams.length).toBeGreaterThanOrEqual(1)
  const legacyTeam = budgetsBody.data.teams.find((team) => team.team_key === 'system-legacy')
  expect(legacyTeam).toBeTruthy()
  const teamId = legacyTeam?.team_id ?? budgetsBody.data.teams[0].team_id

  const upsertBudgetResponse = await request.put(
    `${root}/api/v1/admin/spend/budgets/teams/${teamId}`,
    {
      headers: {
        cookie: adminCookie,
        'content-type': 'application/json',
      },
      data: {
        cadence: 'daily',
        amount_usd: '0.0000',
        hard_limit: true,
        timezone: 'UTC',
      },
    },
  )
  expect(upsertBudgetResponse.status()).toBe(200)

  const clearResponse = await request.delete(stubAdminUrl('/__admin/requests'))
  expect(clearResponse.ok()).toBe(true)

  const blockedResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
      'idempotency-key': 'e2e-team-budget-blocked',
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: 'should be blocked by team budget' }],
    },
  })
  expect(blockedResponse.status()).toBe(429)
  const blockedBody = (await blockedResponse.json()) as {
    error: {
      code: string
    }
  }
  expect(blockedBody.error.code).toBe('budget_exceeded')

  const capturedResponse = await request.get(stubAdminUrl('/__admin/requests'))
  expect(capturedResponse.ok()).toBe(true)
})

test('request log detail returns 404 for a missing row', async ({ request, page, baseURL }) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)

  const response = await request.get(
    `${root}/api/v1/admin/observability/request-logs/00000000-0000-0000-0000-000000000000`,
    {
      headers: {
        cookie: adminCookie,
      },
    },
  )

  expect(response.status()).toBe(404)
  const body = (await response.json()) as {
    error: {
      code: string
      message: string
    }
  }
  expect(body.error.code).toBe('not_found')
  expect(body.error.message).toContain('request log')
})

test('identity users endpoints support live create-and-list flows', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)

  const email = `issue-60-${Date.now()}@example.com`
  const createResponse = await request.post(`${root}/api/v1/admin/identity/users`, {
    headers: {
      cookie: adminCookie,
      'content-type': 'application/json',
    },
    data: {
      name: 'Issue 60 User',
      email,
      auth_mode: 'password',
      global_role: 'user',
    },
  })

  expect(createResponse.status()).toBe(200)
  const createBody = (await createResponse.json()) as {
    data:
      | {
          kind: 'password_invite'
          user: {
            id: string
            email: string
            global_role: string
            status: string
          }
          invite_url: string
        }
      | {
          kind: string
        }
  }

  expect(createBody.data.kind).toBe('password_invite')
  if (createBody.data.kind !== 'password_invite') {
    throw new Error(`expected password invite onboarding, received ${createBody.data.kind}`)
  }
  expect(createBody.data.user.email).toBe(email)
  expect(createBody.data.user.global_role).toBe('user')
  expect(createBody.data.user.status).toBe('invited')
  expect(createBody.data.invite_url).toContain('/admin/invite/')

  const response = await request.get(`${root}/api/v1/admin/identity/users`, {
    headers: {
      cookie: adminCookie,
    },
  })

  expect(response.status()).toBe(200)
  const body = (await response.json()) as {
    data: {
      users: Array<{
        id: string
        email: string
        global_role: string
        status: string
      }>
    }
  }

  expect(
    body.data.users.some(
      (user) =>
        user.id === createBody.data.user.id &&
        user.email === email &&
        user.global_role === 'user' &&
        user.status === 'invited',
    ),
  ).toBe(true)
})
