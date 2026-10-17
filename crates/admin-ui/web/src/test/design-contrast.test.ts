import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, it } from 'vitest'

import {
  GRAPHIC_CONTRAST_MIN,
  TEXT_CONTRAST_MIN,
  dataRoles,
  textRoles,
} from '@/routes/design-system/-components/color-roles'

// Read from disk: Vitest replaces CSS imports, including ?raw ones, with an empty string.
const globalsCss = readFileSync(join(import.meta.dirname, '../styles/globals.css'), 'utf8')
const rootStart = globalsCss.indexOf('\n:root {')
const rootBlock = globalsCss.slice(rootStart, globalsCss.indexOf('\n}', rootStart))
// Every role in color-roles.ts points at a plain hex token on :root.
const tokens = new Map(
  [...rootBlock.matchAll(/--([\w-]+):\s*(#[\da-f]{6});/gi)].map(([, name, hex]) => [name, hex]),
)

function luminance(name: string) {
  const hex = tokens.get(name)
  if (!hex) throw new Error(`--${name} is not a hex token on :root`)
  const [r, g, b] = [1, 3, 5]
    .map((at) => Number.parseInt(hex.slice(at, at + 2), 16) / 255)
    .map((channel) => (channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4))
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrast(background: string, foreground: string) {
  const [light, dark] = [luminance(background), luminance(foreground)].sort((a, b) => b - a)
  return (light + 0.05) / (dark + 0.05)
}

describe('design-system contrast', () => {
  it('reads the colour tokens from globals.css', () => {
    expect(tokens.size).toBeGreaterThan(30)
  })

  it.each(textRoles)('$name text reaches 4.5:1', ({ background, foreground }) => {
    expect(contrast(background, foreground)).toBeGreaterThanOrEqual(TEXT_CONTRAST_MIN)
  })

  it.each(dataRoles)('$name reaches 3:1 on the card', ({ background, foreground }) => {
    expect(contrast(background, foreground)).toBeGreaterThanOrEqual(GRAPHIC_CONTRAST_MIN)
  })

  it('brightens the heatmap from empty to busiest', () => {
    const levels = [0, 1, 2, 3, 4].map((level) => contrast('card', `heatmap-${level}`))
    expect(levels).toEqual([...levels].sort((a, b) => a - b))
  })
})
