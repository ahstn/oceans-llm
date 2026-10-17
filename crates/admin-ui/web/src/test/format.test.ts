import { describe, expect, it } from 'vitest'

import {
  formatCompactCount,
  formatCount,
  formatDuration,
  formatTimestamp,
  formatUsd10000,
  formatUsd10000Precise,
} from '@/lib/format'

// DESIGN.md "Numbers and time": each kind of value has one format.
describe('shared number and time formats', () => {
  it('groups counts and goes compact from 100,000', () => {
    expect(formatCount(9812)).toBe('9,812')
    expect(formatCompactCount(99_999)).toBe('99,999')
    expect(formatCompactCount(100_000)).toBe('100K')
    expect(formatCompactCount(34_356_226)).toBe('34.4M')
    expect(formatCount(null)).toBe('—')
  })

  it('gives durations a unit that scales with size', () => {
    expect(formatDuration(810)).toBe('810 ms')
    expect(formatDuration(3964)).toBe('3.96 s')
    expect(formatDuration(12_450)).toBe('12.4 s')
    expect(formatDuration(59_999)).toBe('59.9 s')
    expect(formatDuration(92_000)).toBe('1 min 32 s')
    expect(formatDuration(3_964_980)).toBe('1 h 6 min')
    expect(formatDuration(7_200_000)).toBe('2 h')
    expect(formatDuration(null)).toBe('—')
  })

  it('keeps one money precision per column', () => {
    expect(formatUsd10000(344_000)).toBe('$34.40')
    expect(formatUsd10000Precise(457)).toBe('$0.0457')
    expect(formatUsd10000Precise(5780)).toBe('$0.5780')
    expect(formatUsd10000Precise(1_982_090)).toBe('$198.2090')
  })

  it('writes timestamps in UTC with the zone, and seconds only on request', () => {
    expect(formatTimestamp('2026-10-16T02:04:09Z')).toBe('16 Oct 2026, 02:04 UTC')
    expect(formatTimestamp('2026-10-16T04:04:09+02:00', { seconds: true })).toBe(
      '16 Oct 2026, 02:04:09 UTC',
    )
    expect(formatTimestamp('2026-09-01T23:00:00Z')).toBe('1 Sep 2026, 23:00 UTC')
    expect(formatTimestamp('not a date')).toBe('—')
    expect(formatTimestamp(null)).toBe('—')
  })
})
