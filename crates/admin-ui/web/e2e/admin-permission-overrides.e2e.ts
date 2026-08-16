import { type APIRequestContext, expect, type Page, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'
import { createActiveRegularUser } from './identity-fixtures'

test.skip(
  process.env.E2E_PERMISSION_SCENARIO !== 'overrides',
  'requires the permission override gateway scenario',
)

test('permission overrides hide pages, resolve empty sets, and deduplicate grants', async ({
  request,
  page,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const teamAdmin = await createActiveRegularUser(request, root, adminCookie, 'override-admin')
  const user = await createActiveRegularUser(request, root, adminCookie, 'override-user')

  await createTeamForAdmin(request, root, adminCookie, teamAdmin.id)

  await expectSessionPermissions(request, root, user.cookie, {
    group: 'users',
    pages: [],
    actions: ['create_api_key', 'update_api_key', 'revoke_api_key'],
    default_page: null,
  })
  await expectSessionPermissions(request, root, teamAdmin.cookie, {
    group: 'team_admins',
    pages: ['api_keys', 'models'],
    actions: ['create_api_key', 'update_api_key', 'revoke_api_key', 'reveal_api_key'],
    default_page: 'api_keys',
  })

  await useSession(page, root, user.cookie)
  await page.goto('/admin/models')
  await expect(page).toHaveURL(/\/admin\/no-access$/)
  await expect(
    page.getByRole('heading', { name: 'No console pages available' }).first(),
  ).toBeVisible()
  await expect(page.getByRole('link', { name: 'Models' })).toHaveCount(0)

  await useSession(page, root, teamAdmin.cookie)
  await page.goto('/admin/identity/teams')
  await expect(page).toHaveURL(/\/admin\/api-keys$/)
  await expect(page.getByRole('link', { name: 'Models' }).first()).toBeVisible()
  await expect(page.getByRole('link', { name: 'Teams' })).toHaveCount(0)
})

type ExpectedPermissions = {
  group: 'team_admins' | 'users'
  pages: string[]
  actions: string[]
  default_page: string | null
}

async function expectSessionPermissions(
  request: APIRequestContext,
  root: string,
  cookie: string,
  expected: ExpectedPermissions,
) {
  const response = await request.get(`${root}/api/v1/auth/session`, { headers: { cookie } })
  expect(response.status()).toBe(200)
  const body = (await response.json()) as { data: { permissions: ExpectedPermissions } }
  expect(body.data.permissions).toEqual(expected)
}

async function createTeamForAdmin(
  request: APIRequestContext,
  root: string,
  adminCookie: string,
  adminUserId: string,
) {
  const response = await request.post(`${root}/api/v1/admin/identity/teams`, {
    headers: { cookie: adminCookie, 'content-type': 'application/json' },
    data: {
      name: `Permission Override ${Date.now()}`,
      admin_user_ids: [adminUserId],
      tags: [],
    },
  })
  expect(response.status()).toBe(200)
}

async function useSession(page: Page, root: string, cookie: string) {
  const separator = cookie.indexOf('=')
  await page.context().clearCookies()
  await page.context().addCookies([
    {
      name: cookie.slice(0, separator),
      value: cookie.slice(separator + 1),
      url: root,
    },
  ])
}
