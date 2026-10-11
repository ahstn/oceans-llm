import fs from 'node:fs/promises'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

import { createProofRecorder } from './proof.mjs'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..')
const requireFromAdminUi = createRequire(path.join(repoRoot, 'crates/admin-ui/web/package.json'))
const { chromium } = requireFromAdminUi('playwright')

const baseURL = requiredEnv('OCEANS_VERIFY_BASE_URL')
const evidenceDir = requiredEnv('OCEANS_VERIFY_EVIDENCE_DIR')
requiredEnv('OCEANS_VERIFY_GATEWAY_VERSION')
const adminEmail = requiredEnv('OCEANS_VERIFY_ADMIN_EMAIL')
const adminPassword = requiredEnv('OCEANS_VERIFY_ADMIN_PASSWORD')
// Seeded local demo user with months of usage history, a weekly budget, and personal keys.
const userEmail = process.env.OCEANS_VERIFY_PROFILE_EMAIL ?? 'alice@platform.local'
const userPassword = process.env.OCEANS_VERIFY_PROFILE_PASSWORD ?? 'localdemo123'
const actions = []
const details = { user: userEmail, actions }
const proof = createProofRecorder({ evidenceDir, file: 'profile-proof.json', featureIds: ['profile'] })
let browser
let page

try {
  browser = await chromium.launch({ headless: true })
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
  page = await context.newPage()
  page.on('pageerror', (error) => console.log(`browser page error: ${error.message}`))

  await signIn(page, userEmail, userPassword)
  await page.waitForURL(/\/admin\/profile(?:\?|$)/, { timeout: 60_000 })
  actions.push({ action: 'sign in lands on profile', result: page.url() })
  proof.pass('sign-in-lands-on-profile', { observed: page.url() })

  const api = await (await page.request.get(`${baseURL}/api/v1/me/profile`)).json()
  const profile = api.data ?? api
  const spent = usd(profile.budget?.spent_usd_10000 ?? 0)
  const limit = usd(profile.budget?.settings?.amount_usd_10000 ?? 0)
  const activeDays = profile.days.filter((day) => day.request_count > 0).length
  details.api = { spent, limit, activeDays, days: profile.days.length }
  if (activeDays < 100) throw new Error(`Expected months of seeded history, found ${activeDays} active days.`)
  proof.pass('seeded-history-present', { observed: activeDays, expected: '>= 100 active days' })

  const link = page.getByRole('link', { name: 'Profile', exact: true }).first()
  await Promise.all([page.waitForURL(/\/admin\/profile(?:\?|$)/), link.click()])
  const meter = page.getByTestId('budget-meter')
  await meter.waitFor()
  const meterText = (await meter.textContent()) ?? ''
  if (!meterText.includes(spent) || !meterText.includes(limit)) {
    throw new Error(`Budget meter "${meterText}" does not show ${spent} of ${limit}.`)
  }
  proof.pass('budget-meter-matches-api', { observed: meterText.trim(), expected: `${spent} of ${limit}` })
  await page.getByTestId('profile-headlines').waitFor()
  await page.locator('header', { hasText: 'Welcome back' }).getByRole('link', { name: 'Manage keys' }).waitFor()
  await settleCharts(page)
  const cells = page.locator('[data-testid="usage-heatmap"] [role="img"][data-level]:not([data-level="0"])')
  const activeCells = await cells.count()
  await cells.last().hover()
  const tooltip = page.getByTestId('heatmap-tooltip')
  await tooltip.waitFor()
  const tooltipText = (await tooltip.textContent()) ?? ''
  for (const label of ['Requests', 'Total tokens', 'Cost']) {
    if (!tooltipText.includes(label)) throw new Error(`Heatmap tooltip is missing ${label}.`)
  }
  proof.pass('heatmap-tooltip-shows-usage', { observed: activeCells })
  await capture(page, '01-profile', true)
  await page.mouse.move(0, 0)

  const showMore = page.getByRole('button', { name: /^Show \d+ more$/ })
  const hasMore = (await showMore.count()) > 0
  if (hasMore) {
    await showMore.click()
    await page.getByRole('button', { name: 'Show fewer' }).waitFor()
  }
  details.page = { meterText, activeCells, tooltipText, expandedKeys: hasMore }
  actions.push({ action: 'open Profile from the sidebar', result: page.url() })
  proof.pass('profile-opens-from-sidebar', { observed: page.url() })

  // Profile links open the create dialog with the viewer preselected as the key owner.
  await page.goto(`${baseURL}/admin/api-keys?create=true`, { waitUntil: 'domcontentloaded' })
  const owner = page.getByRole('dialog', { name: 'Create API key' }).getByRole('combobox', { name: 'Owner user' })
  await owner.waitFor()
  await page.waitForFunction(
    (name) => document.querySelector('[aria-label="Owner user"]')?.textContent?.includes(name),
    'Alice Platform Lead',
    { timeout: 30_000 },
  )
  await page.waitForTimeout(500)
  await capture(page, '02-api-keys-create-prefilled', false)
  const ownerText = (await owner.textContent())?.trim()
  actions.push({ action: 'create link preselects the viewer as owner', result: ownerText })
  proof.pass('create-link-preselects-owner', { observed: ownerText })

  // The system bootstrap admin has no personal keys, so it shows the empty state. It is not a
  // selectable key owner, so only the dialog opening is checked for it.
  await context.clearCookies()
  await signIn(page, adminEmail, adminPassword)
  await page.waitForURL(/\/admin\/profile(?:\?|$)/, { timeout: 60_000 })
  const cta = page.getByRole('link', { name: 'Create your first key' })
  await cta.waitFor()
  await capture(page, '03-profile-empty-keys', false)
  await Promise.all([page.waitForURL(/\/admin\/api-keys\?create=true/), cta.click()])
  const dialog = page.getByRole('dialog', { name: 'Create API key' })
  await dialog.waitFor()
  await page.waitForTimeout(500)
  await capture(page, '04-api-keys-create-dialog', false)
  actions.push({ action: 'empty state opens create dialog', result: page.url() })
  proof.pass('empty-state-opens-create-dialog', { observed: page.url() })

  const written = await proof.write({ details })
  console.log(JSON.stringify(written, null, 2))
} catch (error) {
  if (page) await capture(page, '99-profile-failure', true).catch(() => {})
  await proof.write({ error, details })
  throw error
} finally {
  await browser?.close()
}

async function signIn(target, email, password) {
  await target.goto(`${baseURL}/admin/login`, { waitUntil: 'domcontentloaded' })
  await target.getByRole('heading', { name: 'Sign in' }).waitFor()
  await target.waitForFunction(
    () => {
      const button = Array.from(document.querySelectorAll('button')).find(
        (candidate) => candidate.textContent?.trim() === 'Sign in',
      )
      return button instanceof HTMLButtonElement && !button.disabled
    },
    null,
    { timeout: 60_000 },
  )
  await target.getByLabel('Email').fill(email)
  await target.getByLabel('Password', { exact: true }).fill(password)
  await target.getByRole('button', { name: 'Sign in' }).click()
}

/** Lazy chart chunks load after hydration, then Recharts animates areas in from the left. */
async function settleCharts(target) {
  await target.waitForLoadState('networkidle')
  await target.locator('.recharts-surface').first().waitFor()
  await target.waitForTimeout(2_000)
}

function usd(value10000) {
  return `$${(value10000 / 10_000).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}

async function capture(target, name, fullPage) {
  await target.screenshot({ path: path.join(evidenceDir, `${name}.png`), fullPage })
  const aria = await target.locator('body').ariaSnapshot()
  await fs.writeFile(path.join(evidenceDir, `${name}.aria.txt`), aria)
  proof.artifact(`${name}.png`)
  proof.artifact(`${name}.aria.txt`)
}

function requiredEnv(name) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required.`)
  return value
}
