import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { extname, join, relative } from 'node:path'

import { describe, expect, it } from 'vitest'

// The design-system page renders every installed component, and every part of one that the app
// renders, directly or through a shared component it renders. A file renders a part when it imports the part from its components/ui module and uses it
// as a JSX element. A type-only import, or an import nothing renders, doesn't count; lint only warns
// about unused imports.
const src = join(import.meta.dirname, '..')
const uiDir = join(src, 'components/ui')
const pageDir = join(src, 'routes/design-system')
// The admin shell frames every page, this one included, so the sidebar parts, the inset and
// trigger, and the tooltip provider it renders are on show here too.
const shellFiles = [
  'components/layout/app-shell.tsx',
  'components/app-sidebar.tsx',
  'routes/__root.tsx',
]

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(tsx?|jsx?)$/.test(entry.name) && !/\.test\./.test(entry.name) ? [path] : []
  })
}

function moduleSource(module: string) {
  const file = [`${module}.tsx`, `${module}.ts`].map((name) => join(uiDir, name)).find(existsSync)
  return file ? readFileSync(file, 'utf8') : ''
}

// The components a ui module exports: PascalCase names, so no hooks, variants or constants. In an
// export list, `A as B` exports B.
function exportedComponents(module: string) {
  const source = moduleSource(module)
  const listed = [...source.matchAll(/export\s*\{([^}]*)\}/g)].flatMap(([, list]) =>
    list.split(',').map(
      (specifier) =>
        specifier
          .trim()
          .split(/\s+as\s+/)
          .at(-1) ?? '',
    ),
  )
  const declared = [...source.matchAll(/export\s+(?:function|const)\s+(\w+)/g)].map(
    ([, name]) => name,
  )
  return [...listed, ...declared].filter(
    (name) => /^[A-Z][A-Za-z0-9]*$/.test(name) && /[a-z]/.test(name),
  )
}

// The [module, part] pairs one file renders. In an import list, `A as B` imports A as B.
function partsRenderedIn(source: string): [string, string][] {
  const usedInJsx = (name: string) => new RegExp(`<${name}[\\s/>]`).test(source)
  const staticImports = [
    ...source.matchAll(/import\s+(type\s+)?\{([^}]*)\}\s*from\s*'@\/components\/ui\/([\w-]+)'/g),
  ].filter(([, typeOnly]) => !typeOnly)
  const imported = staticImports.flatMap(([, , specifiers, module]) =>
    specifiers
      .split(',')
      .map((specifier) => specifier.trim())
      .filter((specifier) => specifier && !specifier.startsWith('type '))
      .map((specifier) => [module, ...specifier.split(/\s+as\s+/)]),
  )
  // A lazy import (the chart) destructures its bindings from the module object.
  const lazy = [...source.matchAll(/import\('@\/components\/ui\/([\w-]+)'\)/g)].flatMap(
    ([, module]) => exportedComponents(module).map((part) => [module, part]),
  )
  return [...imported, ...lazy]
    .filter(([, part, local = part]) => usedInJsx(local))
    .map(([module, part]): [string, string] => [module, part])
}

// The ui parts the files render, by module.
function renderedParts(files: string[]) {
  const rendered = new Map<string, Set<string>>()
  for (const [module, part] of files.flatMap((file) =>
    partsRenderedIn(readFileSync(file, 'utf8')),
  )) {
    rendered.set(module, (rendered.get(module) ?? new Set()).add(part))
  }
  return rendered
}

// The parts on show once `parts` render: they, and the parts they render in turn, such as the
// overlay DialogContent renders.
function withNestedParts(module: string, parts: ReadonlySet<string>) {
  const source = moduleSource(module)
  const starts = [...source.matchAll(/^(?:export\s+)?function\s+(\w+)/gm)]
  const bodies = new Map(
    starts.map((match, index) => [match[1], source.slice(match.index, starts[index + 1]?.index)]),
  )
  const shown = new Set(parts)
  for (let grew = true; grew;) {
    grew = false
    for (const part of exportedComponents(module)) {
      const nested = [...shown].some((parent) =>
        new RegExp(`<${part}[\\s/>]`).test(bodies.get(parent) ?? ''),
      )
      if (!shown.has(part) && nested) {
        shown.add(part)
        grew = true
      }
    }
  }
  return shown
}

// The shared components (src/components/shared) the files render. The page shows the ui parts a
// shared component renders whenever it renders that component, as it does AdminTableHeader.
function sharedComponentFiles(files: string[]) {
  const shared = new Set<string>()
  for (const file of files) {
    const source = readFileSync(file, 'utf8')
    for (const [, specifiers, module] of source.matchAll(
      /import\s+\{([^}]*)\}\s*from\s*'@\/components\/shared\/([\w-]+)'/g,
    )) {
      const used = specifiers
        .split(',')
        .map(
          (specifier) =>
            specifier
              .trim()
              .split(/\s+as\s+/)
              .at(-1) ?? '',
        )
        .some((local) => local && new RegExp(`<${local}[\\s/>]`).test(source))
      const path = join(src, 'components/shared', `${module}.tsx`)
      if (used && existsSync(path)) shared.add(path)
    }
  }
  return [...shared]
}

describe('design-system page', () => {
  const pageFiles = sourceFiles(pageDir)
  const page = renderedParts([...pageFiles, ...sharedComponentFiles(pageFiles)])

  it('renders every component in src/components/ui', () => {
    const components = readdirSync(uiDir)
      .filter((file) => ['.ts', '.tsx'].includes(extname(file)) && !/\.test\.tsx?$/.test(file))
      .map((file) => file.slice(0, -extname(file).length))

    expect(components.length).toBeGreaterThan(30)
    expect(components.filter((name) => !page.has(name))).toEqual([])
  })

  it('renders every part of them that the app renders', () => {
    const appFiles = sourceFiles(src).filter(
      (file) => !/^(components\/ui|routes\/design-system|test)\//.test(relative(src, file)),
    )
    const app = renderedParts(appFiles)
    const shell = renderedParts(shellFiles.map((file) => join(src, file)))
    const missing = [...app].flatMap(([module, parts]) => {
      const shown = withNestedParts(module, page.get(module) ?? new Set())
      return [...parts]
        .filter((part) => !shown.has(part) && !shell.get(module)?.has(part))
        .map((part) => `${module}: ${part}`)
    })

    expect(app.size).toBeGreaterThan(25)
    expect(missing).toEqual([])
  })
})
