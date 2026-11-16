import { describe, expect, it } from 'vitest'

import { sparklinePaths } from '@/components/attribution/sparkline-path'

describe('sparkline paths', () => {
  it('closes one area per run, never across a gap', () => {
    const { line, area } = sparklinePaths([[0, 10], [25, 2], null, [75, 2], [100, 10]], 20)
    expect(line).toBe('M0.00,10.00L25.00,2.00M75.00,2.00L100.00,10.00')
    expect(area).toBe(
      'M0.00,10.00L25.00,2.00L25.00,20L0.00,20Z' + 'M75.00,2.00L100.00,10.00L100.00,20L75.00,20Z',
    )
  })

  it('leaves a lone point out of the area', () => {
    expect(sparklinePaths([[0, 5], null, [50, 5], [100, 5]], 10).area).toBe(
      'M50.00,5.00L100.00,5.00L100.00,10L50.00,10Z',
    )
  })
})
