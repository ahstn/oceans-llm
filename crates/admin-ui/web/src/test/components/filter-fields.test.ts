import { describe, expect, it } from 'vitest'

import { dayPicker } from '@/components/data/filter-fields'

describe('dayPicker', () => {
  it('starts the named UTC day at midnight', () => {
    const picker = dayPicker('start')
    expect(picker.toParam?.('2026-11-03')).toBe('2026-11-03T00:00:00.000Z')
    expect(picker.fromParam?.('2026-11-03T00:00:00.000Z')).toBe('2026-11-03')
  })

  it('ends an inclusive day on its last millisecond, for stores that compare with <=', () => {
    const picker = dayPicker('end')
    expect(picker.toParam?.('2026-11-03')).toBe('2026-11-03T23:59:59.999Z')
    expect(picker.fromParam?.('2026-11-03T23:59:59.999Z')).toBe('2026-11-03')
  })

  it('ends an exclusive day at the next midnight, so whole-second < keeps 23:59:59', () => {
    const picker = dayPicker('end-exclusive')
    expect(picker.toParam?.('2026-11-03')).toBe('2026-11-04T00:00:00.000Z')
    expect(picker.toParam?.('2026-12-31')).toBe('2027-01-01T00:00:00.000Z')
    // The chip shows the day the user picked, not the boundary sent to the gateway.
    expect(picker.fromParam?.('2026-11-04T00:00:00.000Z')).toBe('2026-11-03')
  })
})
