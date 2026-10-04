import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { type APIRequestContext, expect, test } from 'playwright/test'

import { ensureAdminSession } from './admin-session'
import { requireEnv } from './env'
import { createActiveRegularUser } from './identity-fixtures'

test.skip(process.env.E2E_SKILLS_ENABLED !== 'true', 'Requires the real Skills S3 test profile')

const fixturePath = (version: number) =>
  fileURLToPath(new URL(`./fixtures/skills/review-code-v${version}.zip`, import.meta.url))

async function claimNamespace(
  request: APIRequestContext,
  root: string,
  cookie: string,
  handle: string,
) {
  const response = await request.post(`${root}/api/v1/skills/namespace`, {
    headers: { cookie },
    data: { handle },
  })
  expect(response.ok()).toBe(true)
  expect((await response.json()).handle).toBe(handle)
}

async function uploadArchive(
  request: APIRequestContext,
  url: string,
  cookie: string,
  version: number,
) {
  return request.post(url, {
    headers: { cookie, 'content-type': 'application/zip' },
    data: await readFile(fixturePath(version)),
  })
}

test('regular users upload, inspect, and select their own skill versions', async ({
  page,
  request,
  baseURL,
}, testInfo) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const owner = await createActiveRegularUser(request, root, adminCookie, 'skills-browser')
  const namespace = `browser-${Date.now().toString(36)}`
  await page.context().clearCookies()
  await page.goto(`${root}/admin/login?redirect=/skills`)
  await page.getByLabel('Email', { exact: true }).fill(owner.email)
  await page.getByLabel('Password', { exact: true }).fill('skills-browser-passw0rd')
  await page.getByRole('button', { name: 'Sign in', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Skills', exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'Upload skill', exact: true }).click()
  const namespaceDialog = page.getByRole('dialog', { name: 'Choose your skill namespace' })
  await namespaceDialog.getByLabel('Namespace', { exact: true }).fill(namespace)
  await namespaceDialog.getByRole('button', { name: 'Claim namespace' }).click()
  const uploadDialog = page.getByRole('dialog', { name: 'Upload a skill', exact: true })
  await uploadDialog.getByLabel('ZIP archive').setInputFiles(fixturePath(1))
  await uploadDialog.getByRole('button', { name: 'Upload skill', exact: true }).click()
  await expect(
    page.getByRole('heading', { name: `${namespace}/review-code`, exact: true }),
  ).toBeVisible()
  await expect(page.getByTestId('skill-instructions')).toContainText('checklist version 1')
  await expect(page.getByText('example-org', { exact: true })).toBeVisible()
  await expect(page.getByText('Upstream version', { exact: true }).locator('..')).toContainText(
    '1.0',
  )
  const source = page.getByRole('link', { name: 'View on GitHub', exact: true })
  await expect(source).toHaveAttribute(
    'href',
    'https://github.com/example-org/review-skills/tree/main/review-code',
  )
  await expect(source).toHaveAttribute('rel', 'noopener noreferrer')
  await page
    .getByRole('navigation', { name: 'Skill files' })
    .getByRole('button', { name: 'references/checklist.md', exact: true })
    .click()
  await expect(page.getByTestId('skill-file-text')).toContainText('Checklist version 1')

  const created = await request.get(`${root}/api/v1/skills/by-name/${namespace}/review-code`, {
    headers: { cookie: owner.cookie },
  })
  expect(created.ok()).toBe(true)
  const createdDetail = await created.json()
  expect(createdDetail.skill.owner_user_id).toBe(owner.id)
  expect(createdDetail.versions[0].sha256).toMatch(/^[a-f0-9]{64}$/)

  await page.getByRole('button', { name: 'Upload new version', exact: true }).click()
  const versionDialog = page.getByRole('dialog', { name: 'Upload a new version', exact: true })
  await versionDialog.getByLabel('ZIP archive').setInputFiles(fixturePath(2))
  await versionDialog.getByRole('button', { name: 'Upload skill', exact: true }).click()
  await expect(page.getByTestId('skill-instructions')).toContainText('checklist version 2')
  await expect(page.getByText('example-org-contributors', { exact: true })).toBeVisible()
  await expect(page.getByText('example-org', { exact: true })).toHaveCount(0)
  await expect(page.getByText('Upstream version', { exact: true }).locator('..')).toContainText(
    '2.0',
  )
  await expect(source).toHaveCount(0)
  await page.getByRole('button', { name: 'Set as default', exact: true }).click()
  await expect(page.getByText('Default version updated', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Set as default', exact: true })).toBeDisabled()
  const saved = await request.get(`${root}/api/v1/skills/${createdDetail.skill.id}`, {
    headers: { cookie: owner.cookie },
  })
  expect(saved.ok()).toBe(true)
  const savedDetail = await saved.json()
  expect(savedDetail.skill.default_version).toBe(2)
  expect(savedDetail.skill.latest_version).toBe(2)
  expect(savedDetail.versions).toHaveLength(2)
  for (const [version, author, github] of [
    [1, 'example-org', 'https://github.com/example-org/review-skills/tree/main/review-code'],
    [2, 'example-org-contributors', 'http://github.com/example-org/review-skills'],
  ] as const) {
    const response = await request.get(
      `${root}/api/v1/skills/${createdDetail.skill.id}/versions/${version}`,
      { headers: { cookie: owner.cookie } },
    )
    expect(response.ok()).toBe(true)
    expect((await response.json()).manifest.metadata).toEqual({
      author,
      version: `${version}.0`,
      github,
    })
  }

  await page.getByRole('combobox', { name: 'Version', exact: true }).click()
  await page.getByRole('option', { name: 'Version 1', exact: true }).click()
  await expect(page.getByTestId('skill-instructions')).toContainText('checklist version 1')
  await expect(page.getByText('example-org', { exact: true })).toBeVisible()
  await expect(page.getByText('Upstream version', { exact: true }).locator('..')).toContainText(
    '1.0',
  )
  await expect(source).toBeVisible()
  await page.getByRole('combobox', { name: 'Version', exact: true }).click()
  await page.getByRole('option', { name: 'Version 2 (default) (latest)', exact: true }).click()
  await expect(page.getByTestId('skill-instructions')).toContainText('checklist version 2')
  await expect(page.getByText('example-org-contributors', { exact: true })).toBeVisible()
  await expect(source).toHaveCount(0)
  const downloadEvent = page.waitForEvent('download')
  await page.getByRole('link', { name: 'Download ZIP', exact: true }).click()
  const download = await downloadEvent
  expect(download.suggestedFilename()).toBe('skill.zip')
  const downloadedPath = await download.path()
  expect(downloadedPath).not.toBeNull()
  expect(
    createHash('sha256')
      .update(await readFile(downloadedPath!))
      .digest('hex'),
  ).toBe(savedDetail.versions.find((item: { version: number }) => item.version === 2).sha256)
  await page.screenshot({ path: testInfo.outputPath('skills-owner-default.png'), fullPage: true })
  await testInfo.attach('skills-accessibility', {
    body: await page.locator('body').ariaSnapshot(),
    contentType: 'text/plain',
  })
})

test('shared reads retain owner-only writes and immutable versions', async ({
  page,
  request,
  playwright,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const adminCookie = await ensureAdminSession(page, request, root)
  const alice = await createActiveRegularUser(request, root, adminCookie, 'skills-alice')
  const bob = await createActiveRegularUser(request, root, adminCookie, 'skills-bob')
  const suffix = Date.now().toString(36)
  const aliceNamespace = `alice-${suffix}`
  const bobNamespace = `bob-${suffix}`
  await claimNamespace(request, root, alice.cookie, aliceNamespace)
  await claimNamespace(request, root, bob.cookie, bobNamespace)

  const cannotRename = await request.post(`${root}/api/v1/skills/namespace`, {
    headers: { cookie: alice.cookie },
    data: { handle: `renamed-${suffix}` },
  })
  expect(cannotRename.status()).toBe(409)

  const firstResponse = await uploadArchive(request, `${root}/api/v1/skills`, alice.cookie, 1)
  expect(firstResponse.ok()).toBe(true)
  const first = await firstResponse.json()
  expect(first.skill.namespace).toBe(aliceNamespace)
  expect(first.skill.owner_user_id).toBe(alice.id)
  expect(first.skill.default_version).toBe(1)

  const duplicateNameResponse = await uploadArchive(request, `${root}/api/v1/skills`, bob.cookie, 1)
  expect(duplicateNameResponse.ok()).toBe(true)
  const duplicateName = await duplicateNameResponse.json()
  expect(duplicateName.skill.id).not.toBe(first.skill.id)
  expect(duplicateName.skill.name).toBe(first.skill.name)
  expect(duplicateName.skill.namespace).toBe(bobNamespace)

  const skillUrl = `${root}/api/v1/skills/${first.skill.id}`
  for (const cookie of [bob.cookie, adminCookie]) {
    expect((await uploadArchive(request, `${skillUrl}/versions`, cookie, 2)).status()).toBe(403)
    const forbiddenDefault = await request.put(`${skillUrl}/default-version`, {
      headers: { cookie },
      data: { version: 1 },
    })
    expect(forbiddenDefault.status()).toBe(403)
    expect((await request.get(skillUrl, { headers: { cookie } })).ok()).toBe(true)
  }

  const nextResponse = await uploadArchive(request, `${skillUrl}/versions`, alice.cookie, 2)
  expect(nextResponse.ok()).toBe(true)
  const next = await nextResponse.json()
  expect(next.skill.latest_version).toBe(2)
  expect(next.skill.default_version).toBe(1)
  const selected = await request.put(`${skillUrl}/default-version`, {
    headers: { cookie: alice.cookie },
    data: { version: 2 },
  })
  expect(selected.ok()).toBe(true)
  expect((await selected.json()).skill.default_version).toBe(2)

  const oldVersion = await request.get(`${skillUrl}/versions/1`, {
    headers: { cookie: bob.cookie },
  })
  expect(oldVersion.ok()).toBe(true)
  expect((await oldVersion.json()).instructions).toContain('checklist version 1')
  const archive = await request.get(`${skillUrl}/versions/1/archive`, {
    headers: { cookie: bob.cookie },
  })
  expect(archive.ok()).toBe(true)
  expect(
    createHash('sha256')
      .update(await archive.body())
      .digest('hex'),
  ).toBe(first.versions[0].sha256)

  const anonymous = await playwright.request.newContext()
  const serviceAccount = await playwright.request.newContext({
    extraHTTPHeaders: { authorization: `Bearer ${requireEnv('E2E_GATEWAY_API_KEY')}` },
  })
  try {
    expect((await anonymous.get(`${root}/api/v1/skills`)).status()).toBe(401)
    expect((await anonymous.get(`${skillUrl}/versions/1/archive`)).status()).toBe(401)
    expect((await serviceAccount.get(skillUrl)).ok()).toBe(true)
    const forbiddenUpload = await serviceAccount.post(`${skillUrl}/versions`, {
      headers: { 'content-type': 'application/zip' },
      data: await readFile(fixturePath(2)),
    })
    expect(forbiddenUpload.status()).toBe(403)
    expect(
      (
        await serviceAccount.post(`${root}/api/v1/skills/namespace`, {
          data: { handle: `bot-${suffix}` },
        })
      ).status(),
    ).toBe(403)
  } finally {
    await anonymous.dispose()
    await serviceAccount.dispose()
  }

  await page.context().clearCookies()
  await page.goto(`${root}/admin/login?redirect=/skills/${first.skill.id}`)
  await page.getByLabel('Email', { exact: true }).fill(bob.email)
  await page.getByLabel('Password', { exact: true }).fill('skills-bob-passw0rd')
  await page.getByRole('button', { name: 'Sign in', exact: true }).click()
  await expect(
    page.getByRole('heading', { name: `${aliceNamespace}/review-code`, exact: true }),
  ).toBeVisible()
  await expect(page.getByTestId('skill-instructions')).toContainText('checklist version 2')
  await expect(page.getByRole('button', { name: 'Upload new version' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Set as default' })).toHaveCount(0)
})

test('multipart server functions reject hostile origins and oversized bodies before dispatch', async ({
  page,
  request,
  baseURL,
}) => {
  const root = baseURL ?? requireEnv('E2E_BASE_URL')
  const cookie = await ensureAdminSession(page, request, root)
  // A nonexistent function makes every probe incapable of changing skill data.
  const functionPath = '/admin/_serverFn/skills-ingress-regression'
  const contentType = 'multipart/form-data; boundary=proof'
  for (const originHeaders of [
    { origin: 'https://hostile.invalid', 'sec-fetch-site': 'same-site' },
    { origin: 'https://hostile.invalid', 'sec-fetch-site': 'cross-site' },
    { origin: 'https://hostile.invalid' },
    {},
  ]) {
    const response = await request.post(`${root}${functionPath}`, {
      headers: { cookie, 'content-type': contentType, ...originHeaders },
      data: '--proof--\r\n',
    })
    expect(response.status()).toBe(403)
    expect(await response.text()).toBe('Forbidden')
  }

  const limitsResponse = await request.get(`${root}/api/v1/skills/limits`, { headers: { cookie } })
  expect(limitsResponse.ok()).toBe(true)
  const limits = await limitsResponse.json()
  const body = Buffer.alloc(limits.max_archive_bytes + 64 * 1024 + 1, 'x')
  const directUi = `http://127.0.0.1:${requireEnv('E2E_UI_PORT')}`
  for (const origin of [root, directUi]) {
    const response = await request.post(`${origin}${functionPath}`, {
      headers: {
        cookie,
        origin,
        'sec-fetch-site': 'same-origin',
        'content-type': contentType,
      },
      data: body,
    })
    expect(response.status()).toBe(413)
    expect(await response.text()).toBe('Multipart upload is too large')
  }
})
