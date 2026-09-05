// Browser steps are sequential because each action changes the page under test.
// oxlint-disable no-await-in-loop
import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'
import { resolve } from 'node:path'
import { chromium } from 'playwright'

const output = resolve(process.env.MCP_DESIGN_SCREENSHOTS ?? 'test-results/mcp-designs')
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1512, height: 1100 } })
page.setDefaultTimeout(10000)
const errors = []
page.on('pageerror', (error) => errors.push(error.message))

async function checkWidth(label) {
  const width = await page.evaluate(() => ({
    client: document.documentElement.clientWidth,
    scroll: document.documentElement.scrollWidth,
  }))
  assert.ok(
    width.scroll <= width.client + 1,
    `${label}: document overflows (${JSON.stringify(width)})`,
  )
}

async function capture(name) {
  await page.screenshot({
    path: resolve(output, `${name}.png`),
    fullPage: true,
    animations: 'disabled',
  })
}

async function verifyCandidate(candidate) {
  await page.goto(`http://127.0.0.1:4317/designs/index.html?candidate=${candidate}`)
  await page.getByRole('heading', { level: 1 }).waitFor()
  await page.evaluate(() => document.fonts.ready)
  await checkWidth(`${candidate} desktop`)
  const masks = await page
    .locator('[data-slot="icon-tile"] > span')
    .evaluateAll((items) => items.map((item) => getComputedStyle(item).maskImage))
  assert.ok(masks.length > 0 && masks.every((mask) => mask !== 'none'), 'Brand masks must render')
  await capture(`${candidate}-dark`)
  await page.getByRole('button', { name: 'Use light theme' }).click()
  await capture(`${candidate}-light`)
  await page.getByRole('button', { name: 'Use dark theme' }).click()
  await page.getByRole('textbox', { name: 'Search servers' }).fill('no-matching-server')
  await page.getByText('No servers to show', { exact: true }).waitFor()
  await page.getByRole('textbox', { name: 'Search servers' }).fill('')
  await page.getByRole('radio', { name: 'Needs attention', exact: true }).click()
  assert.equal(
    await page
      .getByRole('radio', { name: 'Needs attention', exact: true })
      .getAttribute('aria-checked'),
    'true',
  )
  await page.getByRole('radio', { name: 'All servers', exact: true }).click()
  if (candidate === 'registry') {
    await page
      .getByRole('columnheader', { name: 'Server', exact: true })
      .getByRole('button')
      .click()
    assert.ok(
      (await page.getByRole('row').nth(1).innerText()).includes('Cloudflare'),
      'Server name sorting works',
    )
  }
  if (candidate === 'operations') {
    await page
      .getByRole('list', { name: 'Server connections' })
      .getByRole('button', { name: /GitHub/ })
      .click()
    await page.getByRole('region', { name: 'GitHub discovery summary' }).waitFor()
  }
  await page.setViewportSize({ width: 390, height: 844 })
  await checkWidth(`${candidate} mobile`)
  await capture(`${candidate}-mobile`)
  await page.getByRole('button', { name: 'Add server', exact: true }).first().click()
  await page.getByRole('dialog').waitFor()
  await checkWidth(`${candidate} mobile dialog`)
  await page.keyboard.press('Escape')
  await page.setViewportSize({ width: 1512, height: 1100 })
}

async function verifyDialogs() {
  await page.goto('http://127.0.0.1:4317/designs/index.html?candidate=registry')
  await page.getByRole('button', { name: 'Manage', exact: true }).first().click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Tools', exact: true }).click()
  const tool = dialog.getByRole('button', { name: /^search / })
  await tool.click()
  assert.ok((await dialog.locator('pre').innerText()).includes('properties'))
  await capture('server-tools')
  await dialog.getByRole('button', { name: 'Configuration', exact: true }).click()
  await dialog.getByLabel('Display name').fill('GitHub engineering')
  await dialog.getByRole('button', { name: 'Save changes' }).click()
  await dialog.getByRole('heading', { name: 'GitHub engineering', exact: true }).waitFor()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: 'Add server', exact: true }).first().click()
  await dialog
    .getByLabel('Display name')
    .fill('A custom documentation server with a longer display name')
  await dialog
    .getByLabel('Server URL')
    .fill(`https://example.com/${'long-endpoint-'.repeat(35)}/mcp`)
  await dialog.getByRole('button', { name: 'Add server', exact: true }).click()
  await page.getByRole('textbox', { name: 'Search servers' }).fill('A custom documentation')
  await page.getByRole('button', { name: 'Manage', exact: true }).first().click()
  await checkWidth('Long server content')
  await capture('long-server-details')
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: 'Reset sample', exact: true }).click()
  await page.getByRole('button', { name: 'Browse catalog', exact: true }).first().click()
  await dialog.getByRole('heading', { name: 'Server catalog', exact: true }).waitFor()
  await capture('server-catalog')
  assert.equal(
    await dialog.getByRole('button', { name: 'Added GitHub', exact: true }).isDisabled(),
    true,
  )
  await dialog.getByRole('button', { name: 'Add Hugging Face', exact: true }).click()
  await page.getByRole('textbox', { name: 'Search servers' }).fill('Hugging Face')
  assert.ok(
    (await page.getByRole('row').nth(1).innerText()).includes('Hugging Face'),
    'Catalog template adds a server',
  )
  await page.getByRole('button', { name: 'Reset sample', exact: true }).click()
  await page.getByRole('button', { name: 'Refresh GitHub', exact: true }).click()
  assert.equal(
    await page.getByRole('button', { name: 'Refresh GitHub', exact: true }).isDisabled(),
    true,
  )
  await page
    .getByText('Sample discovery complete. Displayed results come from the preview fixtures.')
    .waitFor()
}

try {
  for (const candidate of ['registry', 'library', 'operations']) await verifyCandidate(candidate)
  await verifyDialogs()
  assert.deepEqual(errors, [], 'No browser runtime errors')
  console.log(
    `PASS: three designs, desktop/light/mobile, search/filter/empty, dialogs, schemas, edit/add, long values, refresh. Screenshots: ${output}`,
  )
} finally {
  await browser.close()
}
