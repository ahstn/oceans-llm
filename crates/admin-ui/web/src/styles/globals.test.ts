import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

describe('global design tokens', () => {
  it('contains the preset typography, colors, and radius tokens', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf-8')

    expect(css).toContain("font-family: 'Geist Variable'")
    expect(css).not.toContain('family=Fraunces')
    expect(css).not.toContain('family=Manrope')
    expect(css).toContain('font-size: clamp(15px, 0.28vw + 14px, 16px)')
    expect(css).toContain('--background:')
    expect(css).toContain('--sidebar:')
    expect(css).toContain('--color-surface:')
    expect(css).toContain('--color-card-auth:')
    expect(css).toContain('--color-text-muted:')
    expect(css).toContain('--color-primary-foreground:')
    expect(css).toContain('--shadow-panel:')
    expect(css).toContain('--radius-sm: calc(var(--radius) * 0.6)')
    expect(css).toContain('--radius-md: calc(var(--radius) * 0.8)')
    expect(css).toContain('--radius-lg: var(--radius)')
  })
})
