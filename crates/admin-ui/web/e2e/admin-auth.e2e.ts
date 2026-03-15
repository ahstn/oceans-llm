import { expect, test } from 'playwright/test'

const adminEmail = process.env.E2E_ADMIN_EMAIL ?? 'admin@local'
const adminPassword = process.env.E2E_ADMIN_PASSWORD ?? 'admin'
const newPassword = process.env.E2E_ADMIN_NEW_PASSWORD ?? 's3cur3-passw0rd'

test('bootstrap admin must rotate the password before accessing the control plane', async ({
  page,
}) => {
  await page.goto('/admin/api-keys')

  await expect(page).toHaveURL(/\/admin\/login\?redirect=%2Fapi-keys$/)
  await expect(page.getByText('Admin sign in')).toBeVisible()

  await page.getByLabel('Email').fill(adminEmail)
  await page.getByLabel('Password').fill(adminPassword)

  await Promise.all([
    page.waitForURL(/\/admin\/change-password(?:\?|$)/),
    page.getByRole('button', { name: 'Sign in' }).click(),
  ])

  await expect(page.getByText('Change password')).toBeVisible()
  await page.getByLabel('Current password').fill(adminPassword)
  await page.getByLabel(/^New password$/).fill(newPassword)
  await page.getByLabel(/^Confirm new password$/).fill(newPassword)

  await Promise.all([
    page.waitForURL(/\/admin\/api-keys$/),
    page.getByRole('button', { name: 'Update password' }).click(),
  ])

  await expect(page.getByText('Oceans Gateway')).toBeVisible()
  await expect(page.getByText('Identity onboarding is wired to the gateway.')).toBeVisible()
  await expect(
    page.getByText('Other control-plane pages may still use local preview data.'),
  ).toBeVisible()

  const sessionPayload = await page.evaluate(async () => {
    const response = await fetch('/api/v1/auth/session')
    return response.json()
  })

  expect(sessionPayload.data.must_change_password).toBe(false)
  expect(sessionPayload.data.user.email).toBe(adminEmail)
})
