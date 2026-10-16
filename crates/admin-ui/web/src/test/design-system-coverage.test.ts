import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { extname, join } from 'node:path'

import { describe, expect, it } from 'vitest'

// The design-system page renders every installed component. A component counts as rendered when a
// file under the page imports its module and uses an imported name as a JSX element. A type-only
// import, or an import nothing renders, doesn't count; lint only warns about unused imports.
const src = join(import.meta.dirname, '..')
const uiDir = join(src, 'components/ui')
const pageDir = join(src, 'routes/design-system')

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(tsx?|jsx?)$/.test(entry.name) && !/\.test\./.test(entry.name) ? [path] : []
  })
}

// `A as B` in an import or export list binds B.
function boundName(specifier: string) {
  return (
    specifier
      .trim()
      .split(/\s+as\s+/)
      .at(-1) ?? ''
  )
}

// The PascalCase names a ui module exports. A lazy import (the chart) destructures its bindings
// from the module object, so those are the names that can appear in JSX.
function exportedComponents(module: string) {
  const file = [`${module}.tsx`, `${module}.ts`].map((name) => join(uiDir, name)).find(existsSync)
  const source = file ? readFileSync(file, 'utf8') : ''
  const listed = [...source.matchAll(/export\s*\{([^}]*)\}/g)].flatMap(([, list]) =>
    list.split(','),
  )
  const declared = [...source.matchAll(/export\s+(?:function|const)\s+(\w+)/g)]
  return [...listed.map(boundName), ...declared.map(([, name]) => name)].filter((name) =>
    /^[A-Z]/.test(name),
  )
}

function renderedModules() {
  const rendered = new Set<string>()
  for (const file of sourceFiles(pageDir)) {
    const source = readFileSync(file, 'utf8')
    const usedInJsx = (name: string) => new RegExp(`<${name}[\\s/>]`).test(source)
    const staticImports = source.matchAll(
      /import\s+(type\s+)?\{([^}]*)\}\s*from\s*'@\/components\/ui\/([\w-]+)'/g,
    )
    for (const [, typeOnly, specifiers, module] of staticImports) {
      if (typeOnly) continue
      const names = specifiers
        .split(',')
        .filter((specifier) => specifier.trim() && !specifier.trim().startsWith('type '))
        .map(boundName)
      if (names.some(usedInJsx)) rendered.add(module)
    }
    for (const [, module] of source.matchAll(/import\('@\/components\/ui\/([\w-]+)'\)/g)) {
      if (exportedComponents(module).some(usedInJsx)) rendered.add(module)
    }
  }
  return rendered
}

describe('design-system page', () => {
  it('renders every component in src/components/ui', () => {
    const components = readdirSync(uiDir)
      .filter((file) => ['.ts', '.tsx'].includes(extname(file)) && !/\.test\.tsx?$/.test(file))
      .map((file) => file.slice(0, -extname(file).length))
    const rendered = renderedModules()

    expect(components.length).toBeGreaterThan(30)
    expect(components.filter((name) => !rendered.has(name))).toEqual([])
  })
})
