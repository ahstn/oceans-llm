// Captures 2x screenshots of the locally seeded Oceans admin UI for the showreel.
// Usage: node scripts/capture-admin.mjs <shot-set> [outDir]
//   OCEANS_CAPTURE_BASE_URL   default http://127.0.0.1:38390
//   OCEANS_CAPTURE_EMAIL / OCEANS_CAPTURE_PASSWORD  optional; otherwise the login form's
//   prefilled local-demo admin is submitted as-is.
import fs from 'node:fs/promises'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const projectDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = path.resolve(projectDir, '../..')
const requireFromAdminUi = createRequire(path.join(repoRoot, 'crates/admin-ui/web/package.json'))
const { chromium } = requireFromAdminUi('playwright')

const baseURL = process.env.OCEANS_CAPTURE_BASE_URL ?? 'http://127.0.0.1:38390'
const shotSet = process.argv[2] ?? 'survey'
const outDir = path.resolve(process.argv[3] ?? path.join(projectDir, 'assets/ui/raw'))
const viewport = { width: 1600, height: 900 }

const SETS = {
  survey: [
    { name: 'models', path: '/admin/models' },
    { name: 'spend-controls', path: '/admin/spend-controls' },
    { name: 'request-logs', path: '/admin/observability/request-logs' },
    { name: 'usage-costs', path: '/admin/observability/usage-costs' },
    { name: 'leaderboard', path: '/admin/observability/leaderboard' },
    { name: 'agent-harnesses', path: '/admin/observability/agent-harnesses' },
    { name: 'agent-sessions', path: '/admin/observability/agent-sessions' },
    { name: 'guardrails-settings', path: '/admin/guardrails' },
    { name: 'guardrails-decisions', path: '/admin/observability/guardrails' },
    { name: 'mcp', path: '/admin/mcp' },
    { name: 'mcp-toolsets', path: '/admin/mcp/toolsets' },
    { name: 'api-keys', path: '/admin/api-keys' },
    { name: 'teams', path: '/admin/identity/teams' },
    { name: 'users', path: '/admin/identity/users' },
    { name: 'batches', path: '/admin/batches' },
  ],
  profile: [{ name: 'profile', path: '/admin/profile' }],
  // Open dialogs and sheets so the flight stops land on a single, legible detail.
  refined: [
    { name: 'model-routing-opus', path: '/admin/models', prepare: (page) => openModelRouting(page, 'claude-opus-4-7') },
    { name: 'model-routing-astra', path: '/admin/models', prepare: (page) => openModelRouting(page, 'gpt-6-astra') },
    {
      name: 'spend-increase',
      path: '/admin/spend-controls',
      prepare: async (page) => {
        await page.getByRole('row').filter({ hasText: 'Grace' }).getByRole('button', { name: 'Increase budget' }).click()
        await page.getByRole('dialog').waitFor()
      },
    },
    {
      name: 'request-inspect',
      path: '/admin/observability/request-logs',
      prepare: async (page) => {
        await page.getByRole('button', { name: 'Inspect' }).first().click()
        await page.getByRole('dialog').waitFor()
      },
    },
    { name: 'mcp-invocations', path: '/admin/observability/mcp-invocations' },
  ],
}

async function openModelRouting(page, modelId) {
  await page.getByRole('row').filter({ hasText: modelId }).getByRole('button', { name: 'Info' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Routing' }).click()
}

async function signIn(page) {
  await page.goto(`${baseURL}/admin/login`, { waitUntil: 'domcontentloaded' })
  await page.getByRole('heading', { name: 'Sign in' }).waitFor()
  await page.waitForLoadState('networkidle')
  await page.waitForFunction(() => {
    const button = Array.from(document.querySelectorAll('button')).find(
      (candidate) => candidate.textContent?.trim() === 'Sign in',
    )
    return button instanceof HTMLButtonElement && !button.disabled
  }, null, { timeout: 60_000 })
  if (process.env.OCEANS_CAPTURE_EMAIL) {
    await page.getByLabel('Email').fill(process.env.OCEANS_CAPTURE_EMAIL)
    await page.getByLabel('Password', { exact: true }).fill(process.env.OCEANS_CAPTURE_PASSWORD ?? '')
  }
  await Promise.all([
    page.waitForURL((url) => !url.pathname.endsWith('/login'), { timeout: 60_000 }),
    page.getByRole('button', { name: 'Sign in' }).click(),
  ])
  await page.waitForLoadState('networkidle')
}

async function settle(page, ms = 1800) {
  await page.waitForLoadState('networkidle')
  await page.evaluate(() => document.fonts.ready)
  await page.waitForTimeout(ms)
}

await fs.mkdir(outDir, { recursive: true })
const browser = await chromium.launch({ headless: true })
try {
  const context = await browser.newContext({ viewport, deviceScaleFactor: 2, colorScheme: 'dark' })
  const page = await context.newPage()
  page.on('pageerror', (error) => console.log(`page error: ${error.message}`))
  await signIn(page)
  for (const shot of SETS[shotSet]) {
    await page.goto(`${baseURL}${shot.path}`, { waitUntil: 'domcontentloaded' })
    await settle(page)
    if (shot.prepare) {
      try {
        await shot.prepare(page)
        await page.waitForTimeout(600) // let dialog enter transitions finish
      } catch (error) {
        console.log(`prepare failed for ${shot.name}: ${error.message.split('\n')[0]}`)
      }
    }
    const file = path.join(outDir, `${shot.name}.png`)
    await page.screenshot({ path: file })
    console.log(`captured ${shot.name} -> ${path.relative(projectDir, file)}`)
  }
} finally {
  await browser.close()
}
