import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

describe('global design tokens', () => {
  it('contains required typography, colors, and radius tokens', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf-8')

    expect(css).toContain('family=Fraunces')
    expect(css).toContain('family=Manrope')
    expect(css).toContain('font-size: clamp(15px, 0.28vw + 14px, 16px)')
    expect(css).toContain('--color-surface:')
    expect(css).toContain('--color-card-auth:')
    expect(css).toContain('--color-text-muted:')
    expect(css).toContain('--color-primary-foreground:')
    expect(css).toContain('--shadow-panel:')
    expect(css).toContain('--radius-sm: 6px')
    expect(css).toContain('--radius-md: 8px')
    expect(css).toContain('--radius-lg: 12px')
  })
})
