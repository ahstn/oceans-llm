#!/usr/bin/env node
// Generates the root registry.json (a shadcn `registry:theme` item) from the admin UI's tokens in
// crates/admin-ui/web/src/styles/globals.css, so the published theme never drifts from the app.
//
//   node scripts/build-shadcn-theme.mjs          write registry.json
//   node scripts/build-shadcn-theme.mjs --build  also build docs/public/r with the shadcn CLI
//   node scripts/build-shadcn-theme.mjs --check  exit 1 if registry.json or docs/public/r is out of date
//
// Consumers install it with `npx shadcn@latest add ahstn/oceans-llm/oceans-theme`, or from the
// built item the docs site serves at https://docs.oceans-llm.com/r/oceans-theme.json.
import { spawnSync } from 'node:child_process'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const cssPath = join(root, 'crates/admin-ui/web/src/styles/globals.css')
const registryPath = join(root, 'registry.json')
const outDir = join(root, 'docs/public/r')
const css = readFileSync(cssPath, 'utf8')

// The block body that starts at `selector {` (brace-matched, so nested at-rules stay inside).
function block(source, selector, from = 0) {
  const start = source.indexOf(`${selector} {`, from)
  if (start === -1) throw new Error(`${selector} not found in globals.css`)
  let depth = 0
  for (let i = source.indexOf('{', start); i < source.length; i++) {
    if (source[i] === '{') depth++
    if (source[i] === '}' && --depth === 0) return { body: source.slice(source.indexOf('{', start) + 1, i), end: i }
  }
  throw new Error(`unbalanced block after ${selector}`)
}

function declarations(body) {
  const withoutComments = body.replace(/\/\*[\s\S]*?\*\//g, '')
  const flat = withoutComments.replace(/@keyframes[\s\S]*?\{[\s\S]*?\}\s*\}/g, '')
  return [...flat.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(([, name, value]) => [name, value.trim().replace(/\s+/g, ' ')])
}

function rules(body) {
  return Object.fromEntries(
    [...body.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([\w-]+):\s*([^;]+);/g)].map(([, k, v]) => [k, v.trim()]),
  )
}

// Tokens on :root. Colours go to `dark` (the system is dark only); radius, motion and the panel glow
// apply in every mode, so they go to `light` (which the CLI writes to :root).
const shadcnColours = new Set([
  'background', 'foreground', 'card', 'card-foreground', 'popover', 'popover-foreground', 'primary',
  'primary-foreground', 'secondary', 'secondary-foreground', 'muted', 'muted-foreground', 'accent',
  'accent-foreground', 'destructive', 'border', 'input', 'ring', 'chart-1', 'chart-2', 'chart-3',
  'chart-4', 'chart-5', 'sidebar', 'sidebar-foreground', 'sidebar-primary', 'sidebar-primary-foreground',
  'sidebar-accent', 'sidebar-accent-foreground', 'sidebar-border', 'sidebar-ring',
])
const rootTokens = declarations(block(css, '\n:root').body).filter(
  ([name]) => !name.startsWith('color-') && !name.startsWith('scrollbar-') && !name.startsWith('font-'),
)
const light = {}
const dark = {}
const theme = {
  'font-sans': "'Geist Variable', ui-sans-serif, sans-serif",
  'font-heading': "'Geist Variable', ui-sans-serif, sans-serif",
}
for (const [name, value] of rootTokens) {
  if (name === 'radius' || name.startsWith('duration-') || name === 'panel-glow') {
    light[name] = value
    continue
  }
  dark[name] = value
  // Extra colours need a Tailwind mapping so bg-*, text-* and friends exist in the consuming app.
  if (!shadcnColours.has(name)) theme[`color-${name}`] = `var(--${name})`
}

// Elevation, easing and the page entrance from the `@theme {` block.
const themeBlock = block(css, '\n@theme').body
for (const [name, value] of declarations(themeBlock)) theme[name] = value
// The CLI writes `theme` into `@theme inline`, which may not emit easing variables at runtime, so
// resolve references between theme entries (e.g. the entrance animation's easing) to literals.
for (const [name, value] of Object.entries(theme)) {
  theme[name] = value.replace(/var\(--([\w-]+)\)/g, (match, ref) => (ref in theme && !ref.startsWith('color-') ? theme[ref] : match))
}
const keyframes = block(themeBlock, '@keyframes page-in').body
const fromRules = rules(block(keyframes, 'from').body)

const cssRules = {
  '@keyframes page-in': { from: fromRules },
  '@layer base': {
    '@media (prefers-reduced-motion: reduce)': {
      '*, ::before, ::after': rules(block(css, '@media (prefers-reduced-motion: reduce)').body.replace(/^[\s\S]*?\{/, '').replace(/\}\s*$/, '')),
    },
  },
}
for (const match of css.matchAll(/@utility (type-[\w-]+) \{/g)) {
  cssRules[`@utility ${match[1]}`] = rules(block(css, `@utility ${match[1]}`, match.index).body)
}

const registry = {
  $schema: 'https://ui.shadcn.com/schema/registry.json',
  name: 'oceans',
  homepage: 'https://docs.oceans-llm.com',
  items: [
    {
      name: 'oceans-theme',
      type: 'registry:theme',
      title: 'Oceans Gateway',
      description:
        'The Oceans Gateway admin theme: Reef surfaces, Tokyo Night lilac for actions and Tokyo Night colours for data and status. Dark only.',
      docs: "Dark only: keep class=\"dark\" on <html>, which these tokens target. The type utilities use Geist; install @fontsource-variable/geist and import it, or swap --font-sans. Generated from crates/admin-ui/web/src/styles/globals.css by scripts/build-shadcn-theme.mjs; DESIGN.md describes how to use each token.",
      cssVars: { theme, light, dark },
      css: cssRules,
    },
  ],
}

// The copies the docs site serves come from the shadcn CLI, which reorders keys, so they are compared
// as data: the registry itself, and each item with the item schema the CLI adds.
const published = [
  ['registry.json', registry],
  ...registry.items.map((item) => [
    `${item.name}.json`,
    { $schema: 'https://ui.shadcn.com/schema/registry-item.json', ...item },
  ]),
]

function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys)
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortKeys(value[key])]))
  }
  return value
}

function readText(path) {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return ''
  }
}

function sameData(text, expected) {
  try {
    return JSON.stringify(sortKeys(JSON.parse(text))) === JSON.stringify(sortKeys(expected))
  } catch {
    return false
  }
}

const output = `${JSON.stringify(registry, null, 2)}\n`
if (process.argv.includes('--check')) {
  const stale = readText(registryPath) === output ? [] : ['registry.json']
  for (const [file, expected] of published) {
    if (!sameData(readText(join(outDir, file)), expected)) stale.push(`docs/public/r/${file}`)
  }
  if (stale.length > 0) {
    console.error(`${stale.join(', ')} out of date with globals.css. Run: mise run shadcn-theme`)
    process.exit(1)
  }
  console.log('registry.json and docs/public/r match globals.css')
} else {
  writeFileSync(registryPath, output)
  console.log(`wrote registry.json: ${Object.keys(dark).length} dark tokens, ${Object.keys(theme).length} theme entries`)
  if (process.argv.includes('--build')) {
    const build = spawnSync('npx', ['--yes', 'shadcn@latest', 'build', 'registry.json', '--output', 'docs/public/r'], { cwd: root, stdio: 'inherit' })
    if (build.status !== 0) process.exit(build.status ?? 1)
    // The CLI omits the trailing newline the repository's end-of-file hook requires.
    for (const file of readdirSync(outDir).filter((name) => name.endsWith('.json'))) {
      const path = join(outDir, file)
      const text = readFileSync(path, 'utf8')
      if (!text.endsWith('\n')) writeFileSync(path, `${text}\n`)
    }
  }
}
