// Browser steps are sequential because each action changes the page under test.
// oxlint-disable no-await-in-loop
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { chromium } from 'playwright'

const output = resolve(process.env.TOOLSET_DESIGN_SCREENSHOTS ?? 'test-results/toolsets-revised')
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1697, height: 1235 } })
page.setDefaultTimeout(10000)
const errors = []
const styleEvidence = []
const tooltipOnly = process.argv.includes('--tooltip-only')
page.on('pageerror', (error) => errors.push(error.message))
const baseUrl = 'http://127.0.0.1:4317/designs/toolsets.html?candidate=workbench'
const workbench = page.getByRole('region', { name: 'Tool Sets workbench', exact: true })
const workspace = page.getByRole('region', { name: 'Tool set workspace', exact: true })
const rows = {
  engineering: ['Engineering essentials', 'engineering-essentials', 3],
  research: ['Research desk', 'research-desk', 2],
  support: ['Support knowledge', 'support-knowledge', 2],
  release: ['Release operations', 'release-operations', 3],
  legacy: ['Legacy documentation', 'legacy-docs', 2],
}

function row(id) {
  return page.getByTestId(`toolset-row-${id}`)
}

function checkbox(name) {
  return page.getByRole('checkbox', { name, exact: true })
}

function save(id) {
  return row(id).getByRole('button', { name: `Save ${rows[id][0]}`, exact: true })
}

function dialog(title) {
  return page
    .getByRole('dialog')
    .filter({ has: page.getByRole('heading', { name: title, exact: true }) })
}

async function load() {
  await page.goto(baseUrl)
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).waitFor()
  await page.evaluate(() => document.fonts.ready)
}

async function select(id) {
  await row(id)
    .getByRole('radio', { name: `Select ${rows[id][0]}`, exact: true })
    .click()
  await workspace.getByRole('heading', { name: rows[id][0], exact: true }).waitFor()
}

async function checkWidth(label) {
  const bounds = await page.evaluate(() => {
    const root = document.documentElement
    const nodes = [
      ...document.querySelectorAll(
        '[role="dialog"], [data-slot="card"], [data-testid="toolset-workbench-layout"], [data-testid="mcp-toolset-detail"]',
      ),
    ]
    return [
      { label: 'document', client: root.clientWidth, scroll: root.scrollWidth },
      ...nodes.map((node) => ({
        label: node.getAttribute('data-testid') ?? 'dialog',
        client: node.clientWidth,
        scroll: node.scrollWidth,
      })),
    ]
  })
  assert.ok(
    bounds.every((item) => item.scroll <= item.client + 1),
    `${label}: width overflow ${JSON.stringify(bounds)}`,
  )
}

async function capture(name) {
  await checkWidth(name)
  await page.screenshot({
    path: resolve(output, `${name}.png`),
    fullPage: true,
    animations: 'disabled',
  })
}

async function settleStyles(locator) {
  await locator.evaluate(async (element) => {
    const background = getComputedStyle(element).backgroundColor
    await Promise.all(
      element
        .getAnimations({ subtree: true })
        .map((animation) => animation.finished.catch(() => {})),
    )
    return background
  })
}

async function verifyNavigatorLabels(theme) {
  const alignment = []
  for (const id of Object.keys(rows)) {
    const geometry = await row(id).evaluate((element) => ({
      iconLeft: element.querySelector('[data-slot="icon-tile"]').getBoundingClientRect().left,
      countLeft: element.querySelector('[role="status"]').getBoundingClientRect().left,
    }))
    assert.ok(
      Math.abs(geometry.iconLeft - geometry.countLeft) <= 0.5,
      `${theme}: ${id} tool count aligns with its icon tile`,
    )
    alignment.push({ id, ...geometry })
  }
  const active = row('engineering').getByText('Active', { exact: true })
  await settleStyles(active)
  const colors = await active.evaluate((element) => {
    const probe = document.createElement('span')
    probe.style.backgroundColor = 'var(--color-success-soft)'
    probe.style.color = 'var(--color-success)'
    element.append(probe)
    const expected = getComputedStyle(probe)
    const actual = getComputedStyle(element)
    const result = {
      background: actual.backgroundColor,
      text: actual.color,
      expectedBackground: expected.backgroundColor,
      expectedText: expected.color,
    }
    probe.remove()
    return result
  })
  assert.equal(
    await active.getAttribute('data-variant'),
    'success',
    `${theme}: Active uses the success badge`,
  )
  assert.equal(
    colors.background,
    colors.expectedBackground,
    `${theme}: Active background uses the existing success palette`,
  )
  assert.equal(
    colors.text,
    colors.expectedText,
    `${theme}: Active text uses the existing success palette`,
  )
  const disabled = row('legacy').getByText('Disabled', { exact: true })
  assert.equal(
    await disabled.getAttribute('data-variant'),
    'secondary',
    `${theme}: Disabled remains secondary`,
  )
  assert.notEqual(
    await disabled.evaluate((element) => getComputedStyle(element).backgroundColor),
    colors.background,
    `${theme}: Active and Disabled have distinct badge colors`,
  )
  styleEvidence.push({ theme, kind: 'navigator-labels', alignment, colors })
}

async function buttonSurface(locator) {
  await settleStyles(locator)
  return locator.evaluate((element) => {
    const styles = getComputedStyle(element)
    return {
      background: styles.backgroundColor,
      border: styles.borderTopColor,
      shadow: styles.boxShadow,
      opacity: styles.opacity,
      pointerEvents: styles.pointerEvents,
    }
  })
}

async function verifyActionHover(theme) {
  const selected = row('engineering')
  const edit = selected.getByRole('button', { name: 'Edit Engineering essentials', exact: true })
  const selectedSave = save('engineering')
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).hover()
  const editIdle = await buttonSurface(edit)
  await edit.hover()
  const editHover = await buttonSurface(edit)
  assert.notDeepEqual(editHover, editIdle, `${theme}: Edit has visible hover feedback`)
  const rowBackground = await selected.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )
  await capture(`workbench-edit-hover-${theme}`)
  assert.ok(
    editHover.background !== rowBackground ||
      editHover.border !== editIdle.border ||
      editHover.shadow !== editIdle.shadow,
    `${theme}: Edit hover remains distinct from the selected row`,
  )
  assert.equal(await selectedSave.isDisabled(), true)
  const disabledSave = await buttonSurface(selectedSave)
  assert.equal(
    disabledSave.pointerEvents,
    'none',
    `${theme}: unchanged Save remains non-actionable`,
  )
  assert.ok(Number(disabledSave.opacity) < 1, `${theme}: unchanged Save is visibly disabled`)
  await checkbox('List issues').check()
  assert.equal(await selectedSave.isEnabled(), true)
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).hover()
  const saveIdle = await buttonSurface(selectedSave)
  await selectedSave.hover()
  const saveHover = await buttonSurface(selectedSave)
  assert.notEqual(
    saveHover.background,
    saveIdle.background,
    `${theme}: enabled Save changes color on hover`,
  )
  assert.equal(saveHover.pointerEvents, 'auto', `${theme}: dirty Save is actionable`)
  await capture(`workbench-save-hover-${theme}`)
  await checkbox('List issues').uncheck()
  assert.equal(
    await selectedSave.isDisabled(),
    true,
    `${theme}: reverting changes disables Save again`,
  )
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).hover()
  styleEvidence.push({
    theme,
    kind: 'actions',
    editIdle,
    editHover,
    disabledSave,
    saveIdle,
    saveHover,
  })
}

async function verifyCardAndRowHighlight(theme) {
  const layout = page.getByTestId('toolset-workbench-layout')
  const card = await layout.evaluate((element) => {
    const parent = element.closest('[data-slot="card"]')
    if (!parent) return null
    const styles = getComputedStyle(parent)
    return {
      shadow: styles.boxShadow,
      radius: styles.borderTopLeftRadius,
      hasNavigator: Boolean(parent.querySelector('[aria-label="Tool set navigator"]')),
      hasWorkspace: Boolean(parent.querySelector('[aria-label="Tool set workspace"]')),
    }
  })
  assert.ok(
    card?.hasNavigator && card.hasWorkspace,
    `${theme}: the standard Card contains both workbench panels`,
  )
  assert.ok(
    card.shadow !== 'none' && parseFloat(card.radius) > 0,
    `${theme}: Card retains its standard ring and radius`,
  )
  const transparent = 'rgba(0, 0, 0, 0)'
  const selected = row('engineering')
  const unselected = row('research')
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).hover()
  await settleStyles(selected)
  const selectedBackground = await selected.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )
  assert.notEqual(
    selectedBackground,
    transparent,
    `${theme}: selection background covers the full navigator row`,
  )
  assert.equal(
    await selected
      .getByRole('radio')
      .evaluate((element) => getComputedStyle(element).backgroundColor),
    transparent,
    `${theme}: selected radio has no separate title background`,
  )
  const beforeHover = await unselected.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )
  const bounds = await unselected.boundingBox()
  assert.ok(bounds)
  await unselected.hover({ position: { x: 5, y: bounds.height - 5 } })
  await settleStyles(unselected)
  const hoveredBackground = await unselected.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )
  assert.notEqual(
    hoveredBackground,
    beforeHover,
    `${theme}: hovering the lower row area changes the full row background`,
  )
  assert.notEqual(hoveredBackground, transparent, `${theme}: hovered row background is visible`)
  await unselected.getByRole('radio').hover()
  await settleStyles(unselected)
  assert.equal(
    await unselected.evaluate((element) => getComputedStyle(element).backgroundColor),
    hoveredBackground,
    `${theme}: title hover keeps the full-row surface`,
  )
  assert.equal(
    await unselected
      .getByRole('radio')
      .evaluate((element) => getComputedStyle(element).backgroundColor),
    transparent,
    `${theme}: hovered radio remains transparent`,
  )
  await unselected.getByRole('button', { name: 'Edit Research desk', exact: true }).hover()
  await settleStyles(unselected)
  assert.equal(
    await unselected.evaluate((element) => getComputedStyle(element).backgroundColor),
    hoveredBackground,
    `${theme}: action hover keeps the full-row surface`,
  )
  await capture(`workbench-hover-${theme}`)
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).hover()
  await settleStyles(unselected)
  assert.equal(
    await unselected.evaluate((element) => getComputedStyle(element).backgroundColor),
    beforeHover,
    `${theme}: row highlight clears when hover ends`,
  )
  styleEvidence.push({
    theme,
    card,
    selectedBackground,
    unselectedBackground: beforeHover,
    hoveredBackground,
    innerRadioBackground: transparent,
  })
  await verifyNavigatorLabels(theme)
  await verifyActionHover(theme)
}

async function verifyInitialLayout() {
  await load()
  assert.equal(
    await workbench.getByRole('complementary').count(),
    1,
    'Only the navigator remains as a side panel',
  )
  assert.equal(await workbench.getByText('Your draft', { exact: true }).count(), 0)
  const columns = await page
    .getByTestId('toolset-workbench-layout')
    .evaluate((element) => getComputedStyle(element).gridTemplateColumns.split(' ').length)
  assert.equal(columns, 2, 'Desktop workbench has navigator and catalog columns')
  for (const [id, [name, key, count]] of Object.entries(rows)) {
    await row(id).getByText(`${count} tools`, { exact: true }).waitFor()
    assert.equal(
      await row(id).getByText(key, { exact: true }).count(),
      0,
      `${name}: stable key is absent from navigator`,
    )
    assert.equal(
      await row(id)
        .getByRole('button', { name: `Edit ${name}`, exact: true })
        .count(),
      1,
    )
    assert.equal(await save(id).isDisabled(), true, `${name}: unchanged save is disabled`)
    assert.equal(
      await row(id).locator('button button, [role="radio"] button').count(),
      0,
      'Actions are not nested in the selection control',
    )
  }
  assert.equal(await checkbox('Search repositories').isChecked(), true)
  assert.equal(await checkbox('Get pull request').isChecked(), true)
  assert.equal(await checkbox('Search pages').isChecked(), true)
  assert.equal(await checkbox('List issues').isChecked(), false)
  assert.equal(await checkbox('Legacy code search').isDisabled(), true)
  await verifyCardAndRowHighlight('dark')
  await capture('workbench-dark')
  await page.getByRole('button', { name: 'Use light theme', exact: true }).click()
  await verifyCardAndRowHighlight('light')
  await capture('workbench-light')
  await page.getByRole('button', { name: 'Use dark theme', exact: true }).click()
}

async function verifyDraftsAndSave() {
  await load()
  await checkbox('List issues').check()
  await row('engineering').getByText('4 tools', { exact: false }).waitFor()
  await row('engineering').getByText('· Unsaved', { exact: true }).waitFor()
  assert.equal(await save('engineering').isEnabled(), true)
  await select('research')
  assert.equal(await checkbox('Web search').isChecked(), true)
  assert.equal(await checkbox('Get contents').isChecked(), true)
  await checkbox('Search repositories').check()
  await row('research').getByText('3 tools', { exact: false }).waitFor()
  await select('engineering')
  assert.equal(
    await checkbox('List issues').isChecked(),
    true,
    'Engineering draft survives a set switch',
  )
  await select('research')
  assert.equal(
    await checkbox('Search repositories').isChecked(),
    true,
    'Research draft survives a set switch',
  )
  await save('engineering').click()
  assert.equal(
    await save('engineering').isDisabled(),
    true,
    'A nonselected set can save its own draft',
  )
  assert.equal(await row('engineering').getByText('· Unsaved', { exact: true }).count(), 0)
  assert.equal(await save('research').isEnabled(), true, 'Other drafts remain unsaved')
  await workspace.getByRole('heading', { name: 'Research desk', exact: true }).waitFor()
  await save('research').click()
  assert.equal(await save('research').isDisabled(), true)
  await select('engineering')
  assert.equal(await checkbox('List issues').isChecked(), true, 'Saved membership stays checked')
  await checkbox('List issues').uncheck()
  assert.equal(await save('engineering').isEnabled(), true)
  await checkbox('List issues').check()
  assert.equal(
    await save('engineering').isDisabled(),
    true,
    'Restoring saved membership clears dirty state',
  )
  await capture('workbench-saved')
}

async function verifyMetadataAndKeyboard() {
  await load()
  await row('research').getByRole('button', { name: 'Edit Research desk', exact: true }).click()
  const edit = dialog('Edit tool set details')
  assert.equal(await edit.getByLabel('Display name', { exact: true }).inputValue(), 'Research desk')
  assert.equal(await edit.getByLabel('Key', { exact: true }).inputValue(), 'research-desk')
  assert.equal(await edit.getByLabel('Key', { exact: true }).getAttribute('readonly'), '')
  await edit.getByLabel('Display name', { exact: true }).fill('Research desk revised')
  await edit.getByRole('button', { name: 'Save details', exact: true }).click()
  await edit.waitFor({ state: 'detached' })
  await row('research')
    .getByRole('radio', { name: 'Select Research desk revised', exact: true })
    .waitFor()
  await workspace.getByRole('heading', { name: 'Engineering essentials', exact: true }).waitFor()
  const first = row('engineering').getByRole('radio')
  await first.focus()
  await page.keyboard.press('ArrowDown')
  await page.waitForFunction(
    () => document.activeElement?.getAttribute('aria-label') === 'Select Research desk revised',
  )
  assert.equal(
    await row('research')
      .getByRole('radio')
      .evaluate((element) => element === document.activeElement),
    true,
    'Arrow keys move between set selectors',
  )
  await page.keyboard.press('Space')
  await workspace.getByRole('heading', { name: 'Research desk revised', exact: true }).waitFor()
  assert.equal(await row('research').getByRole('radio').getAttribute('aria-checked'), 'true')
  await row('research')
    .getByRole('button', { name: 'Edit Research desk revised', exact: true })
    .focus()
  await page.keyboard.press('Enter')
  await dialog('Edit tool set details').waitFor()
  await page.keyboard.press('Escape')
}

async function verifyCatalogStates() {
  await load()
  await checkbox('List issues').check()
  await page.getByRole('radio', { name: 'Loading', exact: true }).click()
  await page.getByText('Loading tool catalog…', { exact: true }).waitFor()
  assert.equal(await save('engineering').isDisabled(), true)
  await page.getByRole('radio', { name: 'Failed', exact: true }).click()
  await page.getByText('Catalog failed to load', { exact: true }).waitFor()
  assert.equal(await save('engineering').isDisabled(), true)
  await capture('workbench-catalog-error')
  await page.getByRole('button', { name: 'Retry catalog', exact: true }).click()
  assert.equal(
    await checkbox('List issues').isChecked(),
    true,
    'Catalog failure preserves the draft',
  )
  assert.equal(await save('engineering').isEnabled(), true)
  await select('legacy')
  await page.getByText('Some saved tools are unavailable', { exact: true }).waitFor()
  assert.equal(await checkbox('Legacy code search').isDisabled(), true)
  assert.equal(await checkbox('Legacy code search').isChecked(), true)
  await checkbox('Search repositories').check()
  assert.equal(
    await save('legacy').isDisabled(),
    true,
    'An unavailable saved member blocks replacement',
  )
  await page.getByRole('button', { name: 'Remove Legacy code search', exact: true }).click()
  assert.equal(await checkbox('Legacy code search').isChecked(), false)
  assert.equal(
    await save('legacy').isEnabled(),
    true,
    'Removing the unavailable member enables save',
  )
  await save('legacy').click()
  assert.equal(await save('legacy').isDisabled(), true)
}

async function verifyEmptySelectionAndHandoff() {
  await load()
  for (const name of ['Search repositories', 'Get pull request', 'Search pages'])
    await checkbox(name).uncheck()
  await row('engineering').getByText('0 tools', { exact: false }).waitFor()
  await save('engineering').click()
  const confirm = dialog('Remove all tools?')
  await confirm.getByText('This will remove every tool', { exact: true }).waitFor()
  await confirm.getByRole('button', { name: 'Keep editing', exact: true }).click()
  assert.equal(await save('engineering').isEnabled(), true, 'Cancel keeps the draft unsaved')
  await save('engineering').click()
  await confirm.getByRole('button', { name: 'Remove all tools', exact: true }).click()
  assert.equal(await save('engineering').isDisabled(), true)
  await select('research')
  await select('engineering')
  assert.equal(await checkbox('Search repositories').isChecked(), false)
  await page.getByRole('button', { name: 'Try server handoff', exact: true }).click()
  await page.getByText('2 tools carried from Servers', { exact: true }).waitFor()
  await select('research')
  await row('research').getByText('4 tools', { exact: false }).waitFor()
  assert.equal(await checkbox('Web search').isChecked(), true, 'Handoff retains saved tools')
  assert.equal(
    await checkbox('Get pull request').isChecked(),
    true,
    'Handoff merges imported tools',
  )
  assert.equal(await save('research').isEnabled(), true)
}

async function verifyMobileAndSearch() {
  await load()
  await page.setViewportSize({ width: 390, height: 844 })
  const mobileHeader = await workspace.evaluate((element) => {
    const description = element.querySelector('p').getBoundingClientRect()
    const actions = element.querySelector('button').getBoundingClientRect()
    return {
      width: element.clientWidth,
      descriptionWidth: description.width,
      descriptionBottom: description.bottom,
      actionsTop: actions.top,
    }
  })
  assert.ok(
    mobileHeader.descriptionWidth >= mobileHeader.width * 0.8,
    'Mobile description uses the available width',
  )
  assert.ok(
    mobileHeader.actionsTop >= mobileHeader.descriptionBottom,
    'Mobile actions sit below the set description',
  )
  await verifyCardAndRowHighlight('mobile-dark')
  await capture('workbench-mobile')
  const columns = await page
    .getByTestId('toolset-workbench-layout')
    .evaluate((element) => getComputedStyle(element).gridTemplateColumns.split(' ').length)
  assert.equal(columns, 1, 'Mobile workbench stacks navigator and catalog')
  await page.getByRole('textbox', { name: 'Search tool sets', exact: true }).fill('no-such-set')
  await page.getByText('No matching tool sets', { exact: true }).waitFor()
  await page.getByText('Current set · outside this filter', { exact: true }).waitFor()
  assert.equal(
    await row('engineering')
      .getByRole('button', { name: 'Edit Engineering essentials', exact: true })
      .isVisible(),
    true,
  )
  await checkWidth('mobile empty navigator')
  await page.getByRole('button', { name: 'Reset sample', exact: true }).click()
  assert.equal(
    await page.getByRole('textbox', { name: 'Search tool sets', exact: true }).inputValue(),
    '',
  )
  await page
    .getByRole('textbox', { name: 'Search available tools', exact: true })
    .fill('no-such-tool')
  await page.getByText('No matching tools', { exact: true }).waitFor()
  await page.getByRole('textbox', { name: 'Search available tools', exact: true }).fill('')
  await page.getByRole('button', { name: 'Inspect Search repositories', exact: true }).click()
  assert.ok((await page.locator('pre:visible').innerText()).includes('properties'))
  await capture('workbench-mobile-schema')
  await row('engineering')
    .getByRole('button', { name: 'Edit Engineering essentials', exact: true })
    .click()
  await dialog('Edit tool set details').waitFor()
  await capture('workbench-mobile-edit')
  await page.keyboard.press('Escape')
  await page.getByRole('textbox', { name: 'Search tool sets', exact: true }).fill('no-such-set')
  await page.getByRole('button', { name: 'New tool set', exact: true }).first().click()
  const create = dialog('New tool set')
  await create.getByLabel('Display name', { exact: true }).fill('QA mobile tool set')
  await create.getByLabel('Key', { exact: true }).fill('qa-mobile-toolset')
  await create.getByRole('button', { name: 'Create tool set', exact: true }).click()
  await workspace.getByRole('heading', { name: 'QA mobile tool set', exact: true }).waitFor()
  assert.equal(
    await page.getByRole('button', { name: 'Save QA mobile tool set', exact: true }).isVisible(),
    true,
    'Created set controls stay visible after starting under a filter',
  )
  await checkWidth('mobile created set')
  await page.setViewportSize({ width: 1697, height: 1235 })
}

async function verifyFilteredSelection() {
  await load()
  await checkbox('List issues').check()
  await page.getByRole('textbox', { name: 'Search tool sets', exact: true }).fill('research')
  await page.getByText('Current set · outside this filter', { exact: true }).waitFor()
  assert.equal(
    await row('engineering').isVisible(),
    true,
    'The selected draft stays available outside the search',
  )
  assert.equal(await row('research').isVisible(), true)
  assert.equal(await save('engineering').isEnabled(), true)
  await save('engineering').click()
  assert.equal(await save('engineering').isDisabled(), true)
  await page.getByRole('textbox', { name: 'Search tool sets', exact: true }).fill('')
  await page.getByRole('radio', { name: 'Disabled', exact: true }).click()
  await select('legacy')
  await page.getByRole('radio', { name: 'Active', exact: true }).click()
  await page.getByText('Current set · outside this filter', { exact: true }).waitFor()
  assert.equal(
    await row('legacy')
      .getByRole('button', { name: 'Edit Legacy documentation', exact: true })
      .isEnabled(),
    true,
    'Disabled selected set remains editable outside the active filter',
  )
  await page.getByRole('button', { name: 'Remove Legacy code search', exact: true }).click()
  assert.equal(await save('legacy').isEnabled(), true)
  await save('legacy').click()
  assert.equal(await save('legacy').isDisabled(), true)
}

async function verifySaveTooltip(label) {
  await load()
  const selected = row('engineering')
  const trigger = selected.locator('[data-slot="tooltip-trigger"]')
  const tooltip = page.getByRole('tooltip')
  const message = 'Select or change tools to save changes.'
  assert.equal(await save('engineering').isDisabled(), true)
  await trigger.hover()
  await tooltip.waitFor()
  assert.equal(await tooltip.innerText(), message, `${label}: tooltip explains how to enable Save`)
  const tooltipBounds = await page.locator('[data-slot="tooltip-content"]').boundingBox()
  assert.ok(
    tooltipBounds &&
      tooltipBounds.x >= 0 &&
      tooltipBounds.x + tooltipBounds.width <= page.viewportSize().width,
    `${label}: tooltip stays within the viewport`,
  )
  await capture(`workbench-disabled-save-tooltip-${label}`)
  await workbench.getByRole('heading', { name: 'Tool Sets', exact: true }).hover()
  await selected.getByRole('button', { name: 'Edit Engineering essentials', exact: true }).focus()
  await page.keyboard.press('Tab')
  assert.equal(
    await trigger.evaluate((element) => element === document.activeElement),
    true,
    `${label}: keyboard focus reaches the disabled Save hint`,
  )
  await tooltip.waitFor()
  assert.equal(await tooltip.innerText(), message)
  await page.keyboard.press('Enter')
  await page.keyboard.press('Space')
  assert.equal(
    await save('engineering').isDisabled(),
    true,
    `${label}: hint activation does not enable or execute Save`,
  )
  assert.equal(
    await page.locator('[data-sonner-toast]').count(),
    0,
    `${label}: Enter and Space do not dispatch a save`,
  )
  assert.equal(await page.getByRole('dialog').count(), 0)
  await selected.getByText('3 tools', { exact: true }).waitFor()
  await checkbox('List issues').check()
  assert.equal(await save('engineering').isEnabled(), true)
  assert.equal(
    await selected.locator('[data-slot="tooltip-trigger"]').count(),
    0,
    `${label}: enabled Save has no disabled tooltip trigger`,
  )
  await save('engineering').hover()
  assert.equal(
    await page.getByRole('tooltip').count(),
    0,
    `${label}: enabled Save does not show disabled guidance`,
  )
  await checkWidth(`${label} tooltip transition`)
}

async function verifySaveTooltips() {
  await page.setViewportSize({ width: 1697, height: 1235 })
  await verifySaveTooltip('desktop')
  await page.setViewportSize({ width: 390, height: 844 })
  await verifySaveTooltip('mobile')
  await page.setViewportSize({ width: 1697, height: 1235 })
}

try {
  if (!tooltipOnly) {
    await verifyInitialLayout()
    await verifyDraftsAndSave()
    await verifyMetadataAndKeyboard()
    await verifyCatalogStates()
    await verifyEmptySelectionAndHandoff()
    await verifyFilteredSelection()
    await verifyMobileAndSearch()
  }
  await verifySaveTooltips()
  assert.deepEqual(errors, [], 'No browser runtime errors')
  await writeFile(
    resolve(output, 'verification.json'),
    JSON.stringify(
      {
        passed: true,
        url: baseUrl,
        browserErrors: errors,
        styleEvidence,
        scope: tooltipOnly ? 'disabled-save-tooltip' : 'full-workbench',
        checks: tooltipOnly
          ? [
              'disabled Save pointer tooltip',
              'disabled Save keyboard tooltip',
              'tooltip wrapper cannot save',
              'enabled Save has no disabled tooltip',
              'desktop and mobile tooltip containment',
            ]
          : [
              'standard Card wraps navigator and catalog',
              'full-row selected and hover backgrounds',
              'transparent title selector background',
              'success palette active badges and secondary disabled badges',
              'tool count and icon left alignment',
              'Edit and enabled Save hover feedback',
              'unchanged Save remains disabled',
              'desktop two-column layout',
              'mobile containment',
              'light and dark themes',
              'saved counts and checked tools',
              'per-set drafts and row save',
              'row metadata actions',
              'keyboard selection and separate actions',
              'catalog loading and retry',
              'unavailable member removal',
              'empty-selection confirmation',
              'server handoff merge',
              'schema containment',
              'disabled Save pointer and keyboard tooltip',
              'tooltip wrapper cannot save',
              'enabled Save has no disabled tooltip',
            ],
      },
      null,
      2,
    ),
  )
  console.log(`Revised Workbench browser checks passed. Screenshots: ${output}`)
} finally {
  await browser.close()
}
