#!/usr/bin/env node
// Generates the root registry.json (a shadcn `registry:theme` item) from the admin UI's tokens in
// crates/admin-ui/web/src/styles/globals.css, so the published theme never drifts from the app.
//
//   node scripts/build-shadcn-theme.mjs          write registry.json
//   node scripts/build-shadcn-theme.mjs --build  also build docs/public/r with the shadcn CLI
//   node scripts/build-shadcn-theme.mjs --check  exit 1 if registry.json or docs/public/r is out of date,
//                                                or the docs site's dark theme stops mirroring the tokens
//
// Consumers install it with `npx shadcn@latest add ahstn/oceans-llm/oceans-theme`, or from the
// built item the docs site serves at https://docs.oceans-llm.com/r/oceans-theme.json.
import { spawnSync } from 'node:child_process'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

// Pinned so a CLI release can't change docs/public/r without a change here. To bump it, update this
// and run `mise run shadcn-theme`, then commit what it regenerates.
const SHADCN_VERSION = '4.21.4'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const cssPath = join(root, 'crates/admin-ui/web/src/styles/globals.css')
const registryPath = join(root, 'registry.json')
const outDir = join(root, 'docs/public/r')
const css = readFileSync(cssPath, 'utf8')

// The block body that starts at `selector {` (brace-matched, so nested at-rules stay inside).
function block(source, selector, from = 0) {
  const start = source.indexOf(`${selector} {`, from)
  if (start === -1) throw new Error(`no "${selector.trim()} {" block found`)
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
// apply in every mode, so they go to `light` (which the CLI writes to :root). The scrollbar and font
// tokens stay behind: the scrollbar rule isn't exported, and the theme's fonts below are literal.
const rootTokens = declarations(block(css, '\n:root').body).filter(
  ([name]) => !name.startsWith('scrollbar-') && !name.startsWith('font-'),
)
const light = {}
const dark = {}
// The --color-* aliases are plain variables that components read with var(), such as the badge's
// --color-success-soft. They go in `css` rather than cssVars: the CLI maps every cssVars colour into
// @theme inline, and for a name that already starts with `color-` that mapping points at itself.
const aliases = {}
for (const [name, value] of rootTokens) {
  if (name.startsWith('color-')) aliases[`--${name}`] = value
  else if (name === 'radius' || name.startsWith('duration-') || name === 'panel-glow') light[name] = value
  else dark[name] = value
}

// The CLI writes `theme` into the consumer's `@theme inline`, so it mirrors ours: a Tailwind mapping
// for every colour, the radius scale, and the admin-only utilities (text-subtle-foreground,
// tracking-label, text-2xs, text-display, shadow-sticky-edge, shadow-sticky-edge-end). The fonts are literal because ours
// point at --font-display, which isn't exported.
const theme = {
  'font-sans': "'Geist Variable', ui-sans-serif, sans-serif",
  'font-heading': "'Geist Variable', ui-sans-serif, sans-serif",
  'font-mono': "'Geist Mono Variable', ui-monospace, monospace",
}
for (const [name, value] of declarations(block(css, '\n@theme inline').body)) {
  if (!name.startsWith('font-')) theme[name] = value
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

// The app's :root sets color-scheme, so native controls and scrollbars render dark. A consumer's dark
// mode is the .dark class, so the packaged rule carries it there.
const colorScheme = rules(block(css, '\n:root').body)['color-scheme']
if (!colorScheme) throw new Error('globals.css :root sets no color-scheme')

const cssRules = {
  '@keyframes page-in': { from: fromRules },
  '@layer base': {
    // Under .dark, where the tokens the aliases reference are defined.
    '.dark': { 'color-scheme': colorScheme, ...aliases },
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
      docs: "Dark only: keep class=\"dark\" on <html>, which these tokens target. The type utilities use Geist and code uses Geist Mono; install @fontsource-variable/geist and @fontsource-variable/geist-mono and import them, or swap --font-sans and --font-mono. Generated from crates/admin-ui/web/src/styles/globals.css by scripts/build-shadcn-theme.mjs; DESIGN.md describes how to use each token.",
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

// The docs site's dark theme repeats these admin tokens by hand (VitePress variable: admin token).
// Its light theme, and dark values with no admin counterpart (text-2, text-3, the -2 hover steps and
// the hero gradient), are tuned for the docs and aren't checked.
const docsCssPath = 'docs/.vitepress/theme/custom.css'
const docsDarkMirrors = {
  'vp-c-bg': 'background',
  'vp-c-bg-alt': 'sidebar',
  'vp-c-bg-soft': 'card',
  'vp-c-bg-elv': 'popover',
  'vp-c-gutter': 'sidebar',
  'vp-c-text-1': 'foreground',
  'vp-c-divider': 'border',
  'vp-c-border': 'input',
  'vp-c-brand-1': 'primary',
  'vp-c-brand-3': 'primary',
  'vp-home-hero-name-color': 'primary',
  'vp-button-brand-text': 'primary-foreground',
  'vp-button-brand-hover-text': 'primary-foreground',
  'vp-button-brand-active-text': 'primary-foreground',
  'vp-c-green-1': 'success',
  'vp-c-green-3': 'success',
  'vp-c-red-1': 'destructive',
  'vp-c-red-3': 'destructive',
  'vp-c-yellow-1': 'warning',
  'vp-c-yellow-3': 'warning',
  'vp-c-indigo-1': 'chart-1',
  'vp-c-indigo-3': 'chart-1',
}
// Each soft tint is its colour at a low alpha, so it must keep that colour's channels.
const docsDarkTints = {
  'vp-c-brand-soft': 'primary',
  'vp-c-green-soft': 'success',
  'vp-c-red-soft': 'destructive',
  'vp-c-yellow-soft': 'warning',
  'vp-c-indigo-soft': 'chart-1',
}

// #rrggbb or rgb(r g b / alpha), with alpha as a fraction or a percentage, to [r, g, b, alpha].
function rgba(value = '') {
  const hex = /^#([\da-f]{6})$/i.exec(value)
  if (hex) return [0, 2, 4].map((at) => Number.parseInt(hex[1].slice(at, at + 2), 16)).concat(1)
  const rgb = /^rgb\((\d+) (\d+) (\d+)(?: \/ ([\d.]+)(%?))?\)$/.exec(value)
  if (!rgb) return null
  const alpha = rgb[4] === undefined ? 1 : Number(rgb[4]) / (rgb[5] ? 100 : 1)
  return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), Math.round(alpha * 1000) / 1000]
}

function docsThemeDrift() {
  const docsDark = Object.fromEntries(declarations(block(readText(join(root, docsCssPath)), '\n.dark').body))
  const drift = []
  for (const [variable, token] of Object.entries(docsDarkMirrors)) {
    const [docs, admin] = [rgba(docsDark[variable]), rgba(dark[token])]
    if (!docs || !admin || docs.join() !== admin.join()) drift.push(`--${variable} should be ${dark[token]} (--${token})`)
  }
  for (const [variable, token] of Object.entries(docsDarkTints)) {
    const [docs, admin] = [rgba(docsDark[variable]), rgba(dark[token])]
    if (!docs || !admin || docs.slice(0, 3).join() !== admin.slice(0, 3).join()) {
      drift.push(`--${variable} should be ${dark[token]} (--${token}) at a low alpha`)
    }
  }
  return drift
}

const output = `${JSON.stringify(registry, null, 2)}\n`
if (process.argv.includes('--check')) {
  const stale = readText(registryPath) === output ? [] : ['registry.json']
  for (const [file, expected] of published) {
    if (!sameData(readText(join(outDir, file)), expected)) stale.push(`docs/public/r/${file}`)
  }
  const drift = docsThemeDrift()
  if (stale.length > 0) console.error(`${stale.join(', ')} out of date with globals.css. Run: mise run shadcn-theme`)
  if (drift.length > 0) console.error(`${docsCssPath} .dark no longer mirrors globals.css:\n  ${drift.join('\n  ')}`)
  if (stale.length > 0 || drift.length > 0) process.exit(1)
  console.log('registry.json, docs/public/r and the docs dark theme match globals.css')
} else {
  writeFileSync(registryPath, output)
  console.log(`wrote registry.json: ${Object.keys(dark).length} dark tokens, ${Object.keys(theme).length} theme entries`)
  if (process.argv.includes('--build')) {
    const build = spawnSync('npx', ['--yes', `shadcn@${SHADCN_VERSION}`, 'build', 'registry.json', '--output', 'docs/public/r'], { cwd: root, stdio: 'inherit' })
    if (build.status !== 0) process.exit(build.status ?? 1)
    // The CLI omits the trailing newline the repository's end-of-file hook requires.
    for (const file of readdirSync(outDir).filter((name) => name.endsWith('.json'))) {
      const path = join(outDir, file)
      const text = readFileSync(path, 'utf8')
      if (!text.endsWith('\n')) writeFileSync(path, `${text}\n`)
    }
  }
}
