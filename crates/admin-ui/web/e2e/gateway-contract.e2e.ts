import { type APIRequestContext, expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv, stubAdminUrl } from './env'
import { createActiveRegularUser, invitationToken } from './identity-fixtures'

const gatewayApiKey = process.env.E2E_GATEWAY_API_KEY ?? 'gwk_e2e.secret-value'

type ApiKeysCatalog = {
  users: Array<{ id: string }>
  models: Array<{ key: string }>
}

async function createActiveApiKeyOwner(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
): Promise<{ owner: { id: string }; catalog: ApiKeysCatalog }> {
  const unique = `${Date.now()}-${Math.random().toString(36).slice(2)}`
  const createUserResponse = await request.post(`${root}/api/v1/admin/identity/users`, {
    headers: {
      cookie: adminCookie,
      'content-type': 'application/json',
    },
    data: {
      name: 'E2E All Models Owner',
      email: `all-model-owner-${unique}@example.com`,
      auth_mode: 'password',
      global_role: 'user',
    },
  })
  expect(createUserResponse.status()).toBe(200)
  const createUserBody = (await createUserResponse.json()) as {
    data:
      | {
          kind: 'password_invite'
          user: { id: string }
          invite_url: string
        }
      | {
          kind: string
        }
  }
  expect(createUserBody.data.kind).toBe('password_invite')
  if (createUserBody.data.kind !== 'password_invite') {
    throw new Error(`expected password invite onboarding, received ${createUserBody.data.kind}`)
  }

  const completeInviteResponse = await request.post(
    `${root}/api/v1/auth/invitations/${invitationToken(createUserBody.data.invite_url, root)}/password`,
    {
      headers: {
        'content-type': 'application/json',
      },
      data: {
        password: 'all-model-owner-pass',
      },
    },
  )
  expect(completeInviteResponse.status()).toBe(200)

  const apiKeysResponse = await request.get(`${root}/api/v1/admin/api-keys`, {
    headers: {
      cookie: adminCookie,
    },
  })
  expect(apiKeysResponse.status()).toBe(200)
  const apiKeysBody = (await apiKeysResponse.json()) as {
    data: ApiKeysCatalog
  }
  const owner = apiKeysBody.data.users.find((user) => user.id === createUserBody.data.user.id)
  expect(owner).toBeTruthy()
  if (!owner) {
    throw new Error('expected the activated user to be available as an API key owner')
  }

  return { owner, catalog: apiKeysBody.data }
}

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

  const reportResponse = await request.get(
    `${root}/api/v1/admin/spend/report?days=7&owner_kind=all`,
    {
      headers: {
        cookie: adminCookie,
      },
    },
  )
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

test('service-account budget update triggers hard-limit enforcement for service-account keys', async ({
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
      service_accounts: Array<{ service_account_id: string; service_account_key: string }>
    }
  }
  const serviceAccount = budgetsBody.data.service_accounts.find(
    (item) => item.service_account_key === 'seed-api-keys',
  )
  expect(serviceAccount).toBeTruthy()
  if (!serviceAccount) {
    throw new Error('expected seeded service account key `seed-api-keys`')
  }
  const serviceAccountId = serviceAccount.service_account_id

  const upsertBudgetResponse = await request.put(`${root}/api/v1/admin/spend/budgets`, {
    headers: {
      cookie: adminCookie,
      'content-type': 'application/json',
    },
    data: {
      scope: {
        kind: 'service_account',
        service_account_id: serviceAccountId,
      },
      cadence: 'daily',
      amount_usd: '0.0000',
      hard_limit: true,
      timezone: 'UTC',
    },
  })
  expect(upsertBudgetResponse.status()).toBe(200)

  const clearResponse = await request.delete(stubAdminUrl('/__admin/requests'))
  expect(clearResponse.ok()).toBe(true)

  const blockedResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${gatewayApiKey}`,
      'content-type': 'application/json',
      'idempotency-key': 'e2e-service-account-budget-blocked',
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: 'should be blocked by service account budget' }],
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
  const capturedBody = (await capturedResponse.json()) as { requests: Array<unknown> }
  expect(capturedBody.requests).toHaveLength(0)
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

test('regular identity directory is redacted and identity mutations fail without state changes', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const actor = await createActiveRegularUser(request, root, adminCookie, 'directory-actor')
  const target = await createActiveRegularUser(request, root, adminCookie, 'directory-target')
  const unique = `${Date.now()}-${Math.random().toString(36).slice(2)}`
  const targetId = target.id

  const teamResponse = await request.post(`${root}/api/v1/admin/identity/teams`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: {
      name: `Directory Team ${unique}`,
      admin_user_ids: [actor.id],
      tags: [{ key: 'cost_center', value: 'restricted' }],
    },
  })
  expect(teamResponse.status()).toBe(200)
  const teamBody = (await teamResponse.json()) as {
    data: { id: string; key: string; tags: Array<{ key: string; value: string }> }
  }
  expect(teamBody.data.key).toBeTruthy()
  expect(teamBody.data.tags).toEqual([{ key: 'cost_center', value: 'restricted' }])
  const teamId = teamBody.data.id

  const personalKeyIds: string[] = []
  for (const [name, ownerUserId] of [
    ['Visible personal key', actor.id],
    ['Hidden personal key', target.id],
  ] as const) {
    const keyResponse = await request.post(`${root}/api/v1/admin/api-keys`, {
      headers: { cookie: adminCookie, 'content-type': 'application/json' },
      data: {
        name: `${name} ${unique}`,
        owner_kind: 'user',
        owner_user_id: ownerUserId,
        owner_team_id: null,
        owner_service_account_id: null,
        model_grant_mode: 'all',
        model_keys: [],
      },
    })
    expect(keyResponse.status()).toBe(200)
    const keyBody = (await keyResponse.json()) as { data: { api_key: { id: string } } }
    personalKeyIds.push(keyBody.data.api_key.id)
  }

  const scopedKeysResponse = await request.get(`${root}/api/v1/admin/api-keys`, {
    headers: { cookie: actor.cookie },
  })
  expect(scopedKeysResponse.status()).toBe(200)
  const scopedKeysBody = (await scopedKeysResponse.json()) as {
    data: {
      items: Array<{ id: string }>
      users: Array<{ id: string }>
      service_accounts: unknown[]
      models: Array<{ key: string }>
    }
  }
  expect(scopedKeysBody.data.items.some((item) => item.id === personalKeyIds[0])).toBe(true)
  expect(scopedKeysBody.data.items.some((item) => item.id === personalKeyIds[1])).toBe(false)
  expect(scopedKeysBody.data.users.map((user) => user.id)).toEqual([actor.id])
  expect(scopedKeysBody.data.service_accounts).toEqual([])
  expect(scopedKeysBody.data.models.some((model) => model.key === 'fast')).toBe(true)

  const directoryUsersResponse = await request.get(`${root}/api/v1/identity/directory/users`, {
    headers: { cookie: actor.cookie },
  })
  expect(directoryUsersResponse.status()).toBe(200)
  const directoryUsersBody = (await directoryUsersResponse.json()) as {
    data: { users: Array<Record<string, unknown>> }
  }
  const actorView = directoryUsersBody.data.users.find((user) => user.id === actor.id)
  expect(actorView).toBeTruthy()
  expect(Object.keys(actorView ?? {}).sort()).toEqual([
    'email',
    'global_role',
    'id',
    'name',
    'status',
    'team_id',
    'team_name',
    'team_role',
  ])
  expect(actorView).not.toHaveProperty('auth_mode')
  expect(actorView).not.toHaveProperty('request_logging_enabled')
  expect(actorView).not.toHaveProperty('tags')
  expect(actorView).not.toHaveProperty('onboarding')

  const directoryTeamsResponse = await request.get(`${root}/api/v1/identity/directory/teams`, {
    headers: { cookie: actor.cookie },
  })
  expect(directoryTeamsResponse.status()).toBe(200)
  const directoryTeamsBody = (await directoryTeamsResponse.json()) as {
    data: { teams: Array<Record<string, unknown> & { id: string; members: unknown[] }> }
  }
  const teamView = directoryTeamsBody.data.teams.find((team) => team.id === teamId)
  expect(teamView).toBeTruthy()
  expect(Object.keys(teamView ?? {}).sort()).toEqual([
    'id',
    'member_count',
    'members',
    'name',
    'status',
  ])
  expect(teamView).not.toHaveProperty('key')
  expect(teamView).not.toHaveProperty('tags')
  expect(teamView).not.toHaveProperty('admins')

  for (const adminPath of ['/api/v1/admin/identity/users', '/api/v1/admin/identity/teams']) {
    const response = await request.get(`${root}${adminPath}`, {
      headers: { cookie: actor.cookie },
    })
    expect(response.status()).toBe(403)
  }

  const adminHeaders = { cookie: adminCookie }
  const usersBefore = await (
    await request.get(`${root}/api/v1/admin/identity/users`, { headers: adminHeaders })
  ).json()
  const teamsBefore = await (
    await request.get(`${root}/api/v1/admin/identity/teams`, { headers: adminHeaders })
  ).json()
  const mutationHeaders = { cookie: actor.cookie, 'content-type': 'application/json' }
  const mutations: Array<{
    method: 'post' | 'patch' | 'delete'
    path: string
    data?: Record<string, unknown>
  }> = [
    {
      method: 'post',
      path: '/api/v1/admin/identity/users',
      data: {
        name: 'Forbidden User',
        email: `forbidden-${unique}@example.com`,
        auth_mode: 'password',
        global_role: 'user',
      },
    },
    {
      method: 'patch',
      path: `/api/v1/admin/identity/users/${targetId}`,
      data: { global_role: 'platform_admin' },
    },
    { method: 'post', path: `/api/v1/admin/identity/users/${targetId}/deactivate` },
    { method: 'post', path: `/api/v1/admin/identity/users/${targetId}/reactivate` },
    { method: 'post', path: `/api/v1/admin/identity/users/${targetId}/reset-onboarding` },
    { method: 'post', path: `/api/v1/admin/identity/users/${targetId}/password-invite` },
    {
      method: 'post',
      path: '/api/v1/admin/identity/teams',
      data: { name: 'Forbidden Team', admin_user_ids: [] },
    },
    {
      method: 'patch',
      path: `/api/v1/admin/identity/teams/${teamId}`,
      data: { name: 'Forbidden Rename', admin_user_ids: [actor.id] },
    },
    {
      method: 'post',
      path: `/api/v1/admin/identity/teams/${teamId}/members`,
      data: { user_ids: [targetId] },
    },
    {
      method: 'delete',
      path: `/api/v1/admin/identity/teams/${teamId}/members/${actor.id}`,
    },
    {
      method: 'post',
      path: `/api/v1/admin/identity/teams/${teamId}/members/${actor.id}/transfer`,
      data: { destination_team_id: teamId, destination_role: 'member' },
    },
  ]

  for (const mutation of mutations) {
    const response = await request.fetch(`${root}${mutation.path}`, {
      method: mutation.method,
      headers: mutationHeaders,
      data: mutation.data,
    })
    expect(response.status(), `${mutation.method.toUpperCase()} ${mutation.path}`).toBe(403)
  }

  const usersAfter = await (
    await request.get(`${root}/api/v1/admin/identity/users`, { headers: adminHeaders })
  ).json()
  const teamsAfter = await (
    await request.get(`${root}/api/v1/admin/identity/teams`, { headers: adminHeaders })
  ).json()
  expect(usersAfter.data).toEqual(usersBefore.data)
  expect(teamsAfter.data).toEqual(teamsBefore.data)
})

test('admin ui can create, manage, and revoke an api key that gates live gateway access', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  await ensureAdminSession(page, request, root)

  const keyName = `E2E Live Key ${Date.now()}`

  await page.goto('/admin/api-keys')
  await page.getByRole('button', { name: 'Create API key' }).click()
  await page.getByLabel('Name').fill(keyName)
  await page.getByRole('combobox', { name: 'Owner type' }).click()
  await page.getByRole('option', { name: 'Service account' }).click()
  await page.getByRole('combobox', { name: 'Owner service account' }).click()
  await page.getByRole('option', { name: /Seed API Keys/ }).click()
  await page.getByRole('button', { name: /Select models/ }).click()
  await page.locator('[data-slot="command-item"]').filter({ hasText: /fast/ }).click()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: 'Create API key' }).last().click()

  const rawKey = (await page.getByTestId('new-api-key-raw-key').textContent())?.trim()
  expect(rawKey).toBeTruthy()
  if (!rawKey) {
    throw new Error('expected the create flow to reveal the raw API key once')
  }
  const maskedPrefix = `${rawKey.split('.')[0].slice(0, 12)}****`

  const modelsResponse = await request.get(`${root}/v1/models`, {
    headers: {
      authorization: `Bearer ${rawKey}`,
    },
  })
  expect(modelsResponse.status()).toBe(200)
  const modelsBody = (await modelsResponse.json()) as {
    data: Array<{ id: string }>
  }
  expect(modelsBody.data.map((model) => model.id)).toEqual(['fast'])

  const row = page.locator('tr', { hasText: keyName }).first()
  await expect(row).toBeVisible()
  await expect(row).toContainText(maskedPrefix ?? '')
  await expect(row).toContainText('Seed API Keys')
  await expect(row).not.toContainText('system-legacy')
  await expect(row).toContainText(/\d{4}-\d{2}-\d{2}/)

  await row.getByRole('button', { name: 'Manage' }).click()

  const dialog = page.getByRole('dialog', { name: 'Manage API key' })
  await expect(dialog.getByText(maskedPrefix ?? '')).toBeVisible()
  await expect(dialog.getByText('Seed API Keys')).toBeVisible()
  await expect(dialog).not.toContainText('system-legacy')
  await expect(dialog).toContainText('Never')

  await dialog.getByRole('button', { name: /fast/i }).click()
  await page
    .locator('[data-slot="command-item"]')
    .filter({ hasText: /reasoning/ })
    .click()
  await page.locator('[data-slot="command-item"]').filter({ hasText: /fast/ }).click()
  await page.keyboard.press('Escape')
  await dialog.getByRole('button', { name: 'Save access' }).click()

  await expect(dialog).not.toBeVisible()

  const updatedModelsResponse = await request.get(`${root}/v1/models`, {
    headers: {
      authorization: `Bearer ${rawKey}`,
    },
  })
  expect(updatedModelsResponse.status()).toBe(200)
  const updatedModelsBody = (await updatedModelsResponse.json()) as {
    data: Array<{ id: string }>
  }
  expect(updatedModelsBody.data.map((model) => model.id)).toEqual(['reasoning'])

  await row.getByRole('button', { name: 'Manage' }).click()
  await page.getByRole('button', { name: 'Revoke key' }).click()
  await expect(page.getByRole('dialog', { name: 'Manage API key' })).not.toBeVisible()

  const revokedResponse = await request.get(`${root}/v1/models`, {
    headers: {
      authorization: `Bearer ${rawKey}`,
    },
  })
  expect(revokedResponse.status()).toBe(401)
  const revokedBody = (await revokedResponse.json()) as {
    error: { code: string }
  }
  expect(revokedBody.error.code).toBe('api_key_revoked')
})

test('user-owned all-model api keys track the live model catalog', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)

  const { owner, catalog } = await createActiveApiKeyOwner(request, root, adminCookie)
  const expectedModelKeys = catalog.models.map((model) => model.key).sort()
  expect(expectedModelKeys.length).toBeGreaterThan(0)

  const createResponse = await request.post(`${root}/api/v1/admin/api-keys`, {
    headers: {
      cookie: adminCookie,
      'content-type': 'application/json',
    },
    data: {
      name: `E2E All Models ${Date.now()}`,
      owner_kind: 'user',
      owner_user_id: owner.id,
      owner_team_id: null,
      owner_service_account_id: null,
      model_grant_mode: 'all',
      model_keys: [],
    },
  })
  expect(createResponse.status()).toBe(200)
  const createBody = (await createResponse.json()) as {
    data: {
      api_key: { id: string; model_grant_mode: string; model_keys: string[] }
      raw_key: string
    }
  }
  expect(createBody.data.api_key.model_grant_mode).toBe('all')
  expect(createBody.data.api_key.model_keys).toEqual([])

  const modelsResponse = await request.get(`${root}/v1/models`, {
    headers: {
      authorization: `Bearer ${createBody.data.raw_key}`,
    },
  })
  expect(modelsResponse.status()).toBe(200)
  const modelsBody = (await modelsResponse.json()) as {
    data: Array<{ id: string }>
  }
  expect(modelsBody.data.map((model) => model.id).sort()).toEqual(expectedModelKeys)

  const revokeResponse = await request.post(
    `${root}/api/v1/admin/api-keys/${createBody.data.api_key.id}/revoke`,
    {
      headers: {
        cookie: adminCookie,
      },
    },
  )
  expect(revokeResponse.status()).toBe(200)
})
