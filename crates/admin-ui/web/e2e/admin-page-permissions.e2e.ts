import { type APIRequestContext, expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'
import { type ActiveRegularUser, createActiveRegularUser } from './identity-fixtures'

const sharedPages = [
  'api_keys',
  'models',
  'usage_costs',
  'leaderboard',
  'agent_harnesses',
  'request_logs',
  'mcp_invocations',
  'teams',
  'users',
  'service_accounts',
]

test('resolved groups support shared global reads and team-scoped service accounts', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const teamAdmin = await createActiveRegularUser(request, root, adminCookie, 'permission-admin')
  const member = await createActiveRegularUser(request, root, adminCookie, 'permission-member')
  const otherAdmin = await createActiveRegularUser(request, root, adminCookie, 'permission-other')
  const teamless = await createActiveRegularUser(request, root, adminCookie, 'permission-teamless')

  const primaryTeam = await createTeam(
    request,
    root,
    adminCookie,
    'Permission Primary',
    teamAdmin.id,
  )
  const otherTeam = await createTeam(request, root, adminCookie, 'Permission Other', otherAdmin.id)
  await addTeamMember(request, root, adminCookie, primaryTeam.id, member.id)

  const primaryAccount = await createServiceAccount(
    request,
    root,
    adminCookie,
    primaryTeam.id,
    'Primary Automation',
  )
  await createServiceAccount(request, root, adminCookie, otherTeam.id, 'Other Automation')

  await expectSessionPermissions(request, root, teamAdmin.cookie, 'team_admins')
  await expectSessionPermissions(request, root, member.cookie, 'users')

  const scopedAccounts = await request.get(`${root}/api/v1/admin/identity/service-accounts`, {
    headers: { cookie: member.cookie },
  })
  expect(scopedAccounts.status()).toBe(200)
  const scopedBody = (await scopedAccounts.json()) as {
    data: { service_accounts: Array<{ id: string }>; teams: Array<{ id: string }> }
  }
  expect(scopedBody.data.service_accounts.map((account) => account.id)).toEqual([primaryAccount.id])
  expect(scopedBody.data.teams.map((team) => team.id)).toEqual([primaryTeam.id])

  const teamlessAccounts = await request.get(`${root}/api/v1/admin/identity/service-accounts`, {
    headers: { cookie: teamless.cookie },
  })
  expect(teamlessAccounts.status()).toBe(200)
  expect(await teamlessAccounts.json()).toMatchObject({
    data: { service_accounts: [], teams: [] },
  })

  const forbiddenWrite = await request.post(`${root}/api/v1/admin/identity/service-accounts`, {
    headers: { cookie: member.cookie, 'content-type': 'application/json' },
    data: { team_id: primaryTeam.id, name: 'Forbidden Automation', tags: [] },
  })
  expect(forbiddenWrite.status()).toBe(403)

  await recordUserUsage(request, root, adminCookie, member, 'opencode/1.0')
  await recordUserUsage(request, root, adminCookie, otherAdmin, 'claude-code/1.0')

  const leaderboardResponse = await request.get(
    `${root}/api/v1/admin/observability/leaderboard?range=7d`,
    { headers: { cookie: member.cookie } },
  )
  expect(leaderboardResponse.status()).toBe(200)
  const leaderboardBody = (await leaderboardResponse.json()) as {
    data: {
      leaders: Array<Record<string, unknown> & { user_id: string }>
    }
  }
  const visibleUserIds = leaderboardBody.data.leaders.map((leader) => leader.user_id)
  expect(visibleUserIds).toContain(member.id)
  expect(visibleUserIds).toContain(otherAdmin.id)
  expect(leaderboardBody.data.leaders.find((leader) => leader.user_id === otherAdmin.id)).toEqual(
    expect.objectContaining({
      user_name: otherAdmin.name,
      total_requests: expect.any(Number),
      total_spend_usd_10000: expect.any(Number),
      most_used_model: 'fast',
      tool_cardinality_averages: expect.any(Object),
    }),
  )

  const harnessResponse = await request.get(
    `${root}/api/v1/admin/observability/harness-usage?range=7d`,
    { headers: { cookie: member.cookie } },
  )
  expect(harnessResponse.status()).toBe(200)
  const harnessBody = (await harnessResponse.json()) as {
    data: { leaders: Array<{ agent_harness_key: string }> }
  }
  expect(harnessBody.data.leaders.map((leader) => leader.agent_harness_key)).toEqual(
    expect.arrayContaining(['opencode', 'claude_code']),
  )
})

async function createTeam(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
  name: string,
  adminUserId: string,
) {
  const response = await request.post(`${root}/api/v1/admin/identity/teams`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: { name: `${name} ${Date.now()}`, admin_user_ids: [adminUserId], tags: [] },
  })
  expect(response.status()).toBe(200)
  const body = (await response.json()) as { data: { id: string } }
  return body.data
}

async function addTeamMember(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
  teamId: string,
  userId: string,
) {
  const response = await request.post(`${root}/api/v1/admin/identity/teams/${teamId}/members`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: { user_ids: [userId] },
  })
  expect(response.status()).toBe(200)
}

async function createServiceAccount(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
  teamId: string,
  name: string,
) {
  const response = await request.post(`${root}/api/v1/admin/identity/service-accounts`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: { team_id: teamId, name, tags: [] },
  })
  expect(response.status()).toBe(200)
  const body = (await response.json()) as { data: { id: string } }
  return body.data
}

async function expectSessionPermissions(
  request: APIRequestContext,
  root: string,
  cookie: string,
  group: 'team_admins' | 'users',
) {
  const response = await request.get(`${root}/api/v1/auth/session`, { headers: { cookie } })
  expect(response.status()).toBe(200)
  const body = (await response.json()) as {
    data: { permissions: { group: string; pages: string[]; default_page: string } }
  }
  expect(body.data.permissions).toEqual({
    group,
    pages: sharedPages,
    default_page: 'usage_costs',
  })
}

async function recordUserUsage(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
  user: ActiveRegularUser,
  userAgent: string,
) {
  const budgetResponse = await request.put(`${root}/api/v1/admin/spend/budgets`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: {
      scope: { kind: 'user', user_id: user.id },
      cadence: 'daily',
      amount_usd: '100.0000',
      hard_limit: true,
      timezone: 'UTC',
    },
  })
  expect(budgetResponse.status()).toBe(200)

  const keyResponse = await request.post(`${root}/api/v1/admin/api-keys`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: {
      name: `${user.name} Key`,
      owner_kind: 'user',
      owner_user_id: user.id,
      owner_team_id: null,
      owner_service_account_id: null,
      model_grant_mode: 'all',
      model_keys: [],
    },
  })
  expect(keyResponse.status()).toBe(200)
  const keyBody = (await keyResponse.json()) as { data: { raw_key: string } }

  const completionResponse = await request.post(`${root}/v1/chat/completions`, {
    headers: {
      authorization: `Bearer ${keyBody.data.raw_key}`,
      'content-type': 'application/json',
      'user-agent': userAgent,
    },
    data: {
      model: 'fast',
      messages: [{ role: 'user', content: `usage for ${user.name}` }],
    },
  })
  expect(completionResponse.status()).toBe(200)
}
