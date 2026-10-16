import { readdirSync, readFileSync } from 'node:fs'
import { extname, join } from 'node:path'

import { describe, expect, it } from 'vitest'

// The design-system page shows every installed component. A component counts as shown when a file
// under the page imports it, the same rule as the shadcn skill's showcase-coverage.mjs.
const src = join(import.meta.dirname, '..')
const uiDir = join(src, 'components/ui')
const pageDir = join(src, 'routes/design-system')

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(tsx?|jsx?)$/.test(entry.name) ? [path] : []
  })
}

describe('design-system page', () => {
  it('shows every component in src/components/ui', () => {
    const components = readdirSync(uiDir)
      .filter((file) => ['.ts', '.tsx'].includes(extname(file)) && !/\.test\.tsx?$/.test(file))
      .map((file) => file.slice(0, -extname(file).length))
    const source = sourceFiles(pageDir)
      .map((file) => readFileSync(file, 'utf8'))
      .join('\n')
    const missing = components.filter((name) => !new RegExp(`/ui/${name}['"]`).test(source))

    expect(components.length).toBeGreaterThan(30)
    expect(missing).toEqual([])
  })
})
