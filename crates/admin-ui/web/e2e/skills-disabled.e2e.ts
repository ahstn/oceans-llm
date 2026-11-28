import { expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'

test.skip(process.env.E2E_SKILLS_ENABLED === 'true', 'Requires the default disabled Skills setup')

test('disabled Skills shows setup steps and keeps the admin navigation', async ({
  page,
  request,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  await ensureAdminSession(page, request, root)
  await page.goto(`${root}/admin/skills`)
  await expect(page.getByText('Skills storage is off', { exact: true })).toBeVisible()
  await expect(page.getByRole('link', { name: 'Set up skills storage', exact: true })).toBeVisible()
  // A retry can't succeed until the gateway config changes, so the setup state offers none.
  await expect(page.getByRole('button', { name: 'Try again', exact: true })).toHaveCount(0)
  await page.getByRole('link', { name: 'Profile', exact: true }).click()
  await expect(page).toHaveURL(/\/admin\/profile$/)
  await expect(page.getByText('Skills storage is off', { exact: true })).toHaveCount(0)
})
