import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, it } from 'vitest'

import { chartAnimation } from '@/lib/chart-motion'

// Read from disk: Vitest replaces CSS imports, including ?raw ones, with an empty string.
const globalsCss = readFileSync(join(import.meta.dirname, '../styles/globals.css'), 'utf8')

describe('chartAnimation', () => {
  it('matches the --duration-chart token, so the CSS stays the one source of the duration', () => {
    const token = /--duration-chart:\s*(\d+)ms;/.exec(globalsCss)?.[1]

    expect(token).toBeDefined()
    expect(chartAnimation.animationDuration).toBe(Number(token))
  })
})
