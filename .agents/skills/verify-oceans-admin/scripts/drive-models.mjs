import fs from 'node:fs/promises'
import path from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..')
const requireFromAdminUi = createRequire(path.join(repoRoot, 'crates/admin-ui/web/package.json'))
const { chromium } = requireFromAdminUi('playwright')

const baseURL = requiredEnv('OCEANS_VERIFY_BASE_URL')
const evidenceDir = requiredEnv('OCEANS_VERIFY_EVIDENCE_DIR')
const gatewayVersion = requiredEnv('OCEANS_VERIFY_GATEWAY_VERSION')
const email = requiredEnv('OCEANS_VERIFY_ADMIN_EMAIL')
const password = requiredEnv('OCEANS_VERIFY_ADMIN_PASSWORD')
const actions = []

await fs.mkdir(evidenceDir, { recursive: true })
const browser = await chromium.launch({ headless: true })
let page

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
  page = await context.newPage()
  page.on('console', (message) => console.log(`browser console ${message.type()}: ${message.text()}`))
  page.on('pageerror', (error) => console.log(`browser page error: ${error.message}`))
  page.on('requestfailed', (request) =>
    console.log(`browser request failed: ${request.method()} ${request.url()} ${request.failure()?.errorText ?? ''}`),
  )

  await page.goto(`${baseURL}/admin/api-keys`, { waitUntil: 'domcontentloaded' })
  await page.getByRole('heading', { name: 'Sign in' }).waitFor()
  const signInButton = page.getByRole('button', { name: 'Sign in' })
  try {
    await signInButton.waitFor({ state: 'visible', timeout: 60_000 })
    await page.waitForFunction(() => {
      const button = Array.from(document.querySelectorAll('button')).find(
        (candidate) => candidate.textContent?.trim() === 'Sign in',
      )
      return button instanceof HTMLButtonElement && !button.disabled
    }, null, { timeout: 60_000 })
  } catch {
    console.log('Sign-in did not hydrate after the first Vite load; reloading once.')
    await page.reload({ waitUntil: 'domcontentloaded' })
    await page.getByRole('heading', { name: 'Sign in' }).waitFor()
    await page.waitForFunction(() => {
      const button = Array.from(document.querySelectorAll('button')).find(
        (candidate) => candidate.textContent?.trim() === 'Sign in',
      )
      return button instanceof HTMLButtonElement && !button.disabled
    }, null, { timeout: 60_000 })
  }
  actions.push({ action: 'open protected admin UI', result: page.url() })
  await capture(page, '01-login')

  await page.getByLabel('Email').fill(email)
  await page.getByLabel('Password', { exact: true }).fill(password)
  await Promise.all([
    page.waitForURL(/\/admin\/(?:api-keys|models)(?:\?|$)/),
    signInButton.click(),
  ])
  actions.push({ action: 'sign in with seeded platform admin', result: page.url() })

  const modelsLink = page.getByRole('link', { name: 'Models' }).first()
  await modelsLink.waitFor()
  await Promise.all([page.waitForURL(/\/admin\/models(?:\?|$)/), modelsLink.click()])

  await page.getByRole('heading', { name: 'Models', exact: true }).waitFor()
  await page.getByRole('heading', { name: 'Model list' }).waitFor()
  await page.getByTestId('models-desktop-table').waitFor()
  await page.getByTestId('models-desktop-cell-gpt-5.6-sol').waitFor()
  const showingText = (await page.getByText(/^Showing \d+ of \d+ models$/).textContent())?.trim()
  if (!showingText) throw new Error('The Models page did not report a visible model count.')
  const match = /^Showing (\d+) of (\d+) models$/.exec(showingText)
  if (!match) throw new Error(`Unexpected Models count text: ${showingText}`)
  const displayedCount = Number(match[1])
  const totalCount = Number(match[2])
  const renderedCount = await page.locator('[data-testid^="models-desktop-cell-"]').count()
  if (displayedCount !== renderedCount) {
    throw new Error(`UI reported ${displayedCount} displayed models but rendered ${renderedCount} model rows.`)
  }
  actions.push({ action: 'follow Models sidebar link', result: showingText })
  await capture(page, '02-models')

  const adminModels = await page.evaluate(async () => {
    const response = await fetch('/api/v1/admin/models?page=1&page_size=100')
    if (!response.ok) throw new Error(`admin models request returned ${response.status}`)
    return response.json()
  })
  const apiCount = adminModels.data.total
  if (apiCount !== totalCount) {
    throw new Error(`UI total model count ${totalCount} did not match admin API count ${apiCount}.`)
  }

  const modelCell = page.getByTestId('models-desktop-cell-gpt-5.6-sol')
  const modelRow = modelCell.locator('xpath=ancestor::tr')
  await modelRow.getByRole('button', { name: 'Info' }).click()
  const infoDialog = page.getByRole('dialog', { name: 'Model info' })
  await infoDialog.getByText('gpt-5.6-sol', { exact: true }).first().waitFor()
  const infoSections = infoDialog.getByRole('navigation', { name: 'Model info sections' })
  for (const name of ['Overview', 'Routing', 'Economics', 'Access']) {
    await infoSections.getByRole('button', { name, exact: true }).click()
    await infoDialog.getByRole('heading', { name, exact: true }).waitFor()
  }
  actions.push({ action: 'inspect gpt-5.6-sol model info', result: 'All platform-admin sections visible' })
  await capture(page, '03-model-info')
  await page.keyboard.press('Escape')
  await infoDialog.waitFor({ state: 'hidden' })

  await page.getByRole('button', { name: 'Columns', exact: true }).click()
  await page.getByRole('checkbox', { name: /^Context window/ }).check()
  await page.getByRole('columnheader', { name: 'Context window', exact: true }).waitFor()
  await page.getByRole('checkbox', { name: /^Capabilities/ }).check()
  await page.getByRole('columnheader', { name: 'Capabilities', exact: true }).waitFor()
  await page.keyboard.press('Escape')
  actions.push({ action: 'enable optional model columns', result: 'Context window and Capabilities visible' })
  await capture(page, '04-model-columns')

  const configRow = page
    .getByTestId('models-desktop-cell-gpt-5.6-sol')
    .locator('xpath=ancestor::tr')
  await configRow
    .getByRole('button', { name: 'Generate client config for gpt-5.6-sol', exact: true })
    .click()
  const configDialog = page.getByRole('dialog', { name: 'Client config' })
  await configDialog.waitFor()
  await configDialog.getByText(/^gpt-5\.6-sol via /).waitFor()
  const clientConfigs = await verifyClientConfigs(page, configDialog)
  actions.push({ action: 'generate gpt-5.6-sol client config', result: 'Client config dialog visible' })
  await capture(page, '05-model-client-config')

  const proof = {
    feature: 'models',
    entryUrl: `${baseURL}/admin/api-keys`,
    finalUrl: page.url(),
    gatewayVersion,
    modelId: 'gpt-5.6-sol',
    displayedCount,
    renderedCount,
    totalCount,
    apiCount,
    clientConfigs,
    actions,
    generatedAt: new Date().toISOString(),
  }
  await fs.writeFile(path.join(evidenceDir, 'models-proof.json'), `${JSON.stringify(proof, null, 2)}\n`)
  console.log(`models proof passed: ${displayedCount} displayed and ${totalCount} total UI models matched the rendered rows and API total`)
  console.log(`evidence: ${evidenceDir}`)
} catch (error) {
  if (page && !page.isClosed()) {
    await capture(page, '99-models-failure').catch(() => {})
  }
  throw error
} finally {
  await browser.close()
}

async function verifyClientConfigs(page, dialog) {
  const response = await page.evaluate(async () => {
    const result = await fetch('/api/v1/admin/models/client-configs', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ model_keys: ['gpt-5.6-sol'] }),
    })
    if (!result.ok) throw new Error(`Client configuration API returned ${result.status}`)
    return result.json()
  })
  const configs = response.data.client_configurations
  if (configs.length === 0) throw new Error('Client configuration API returned no configurations.')
  const choices = dialog.getByRole('radiogroup', { name: 'Client config', exact: true })
  const results = []
  for (const config of configs) {
    await choices.getByRole('radio', { name: config.label, exact: true }).check()
    for (const block of config.blocks) {
      const rendered = dialog.locator('[data-slot="code-block"]').filter({
        has: page.getByText(block.filename, { exact: true }),
      })
      await rendered.waitFor()
      const content = await rendered.locator('[data-slot="code-block-line"]').evaluateAll((lines) =>
        lines.map((line) => line.lastElementChild.textContent.replace(/\u200b/g, '')).join('\n'),
      )
      if (content !== block.content.replace(/\r\n?/g, '\n')) {
        throw new Error(`Rendered ${config.label} ${block.filename} did not match the configuration API.`)
      }
    }
    await capture(page, `05-model-client-config-${config.key}`)
    results.push({ key: config.key, label: config.label, files: config.blocks.map((block) => block.filename), matchesApi: true })
  }
  await choices.getByRole('radio', { name: configs[0].label, exact: true }).check()
  actions.push({ action: 'compare rendered client configurations with production API', result: `${results.length} configurations matched` })
  return results
}

async function capture(page, name) {
  await page.screenshot({ path: path.join(evidenceDir, `${name}.png`), fullPage: true })
  const snapshot = await page.locator('body').ariaSnapshot()
  await fs.writeFile(path.join(evidenceDir, `${name}.aria.txt`), `${snapshot}\n`)
}

function requiredEnv(name) {
  const value = process.env[name]
  if (!value) throw new Error(`Missing required environment variable ${name}`)
  return value
}
