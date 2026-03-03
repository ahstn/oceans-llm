import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

describe('global design tokens', () => {
  it('contains required typography, colors, and radius tokens', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf-8')

    expect(css).toContain('font-size: 14px')
    expect(css).toContain('--color-bg: #1c1c1c')
    expect(css).toContain('--color-text: #e5e5e5')
    expect(css).toContain('--color-subtle: #a1a1a1')
    expect(css).toContain('oklch(72.3% 0.219 149.579)')
    expect(css).toContain('--radius-sm: 6px')
    expect(css).toContain('--radius-md: 8px')
    expect(css).toContain('--radius-lg: 12px')
  })
})
