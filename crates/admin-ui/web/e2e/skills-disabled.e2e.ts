import { expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'

test.skip(process.env.E2E_SKILLS_ENABLED === 'true', 'Requires the default disabled Skills setup')

test('disabled Skills keeps the admin navigation available', async ({ page, request, baseURL }) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  await ensureAdminSession(page, request, root)
  await page.goto(`${root}/admin/skills`)
  await expect(
    page.getByRole('heading', { name: 'Skills are unavailable', exact: true }),
  ).toBeVisible()
  await expect(page.getByRole('alert')).toContainText('skills are not configured')
  await expect(page.getByRole('button', { name: 'Try again', exact: true })).toBeVisible()
  await page.getByRole('link', { name: 'Profile', exact: true }).click()
  await expect(page).toHaveURL(/\/admin\/profile$/)
  await expect(page.getByRole('heading', { name: 'Skills are unavailable' })).toHaveCount(0)
})
