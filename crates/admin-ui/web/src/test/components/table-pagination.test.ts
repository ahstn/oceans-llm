import { describe, expect, it } from 'vitest'

import { pageSlots } from '@/components/data/table-pagination'

describe('pageSlots', () => {
  it('lists every page when they fit', () => {
    expect(pageSlots(1, 1)).toEqual([1])
    expect(pageSlots(4, 7)).toEqual([1, 2, 3, 4, 5, 6, 7])
  })

  it('keeps the first and last page with an ellipsis for each hidden stretch', () => {
    expect(pageSlots(1, 177)).toEqual([1, 2, 3, 4, 5, 'ellipsis-end', 177])
    expect(pageSlots(4, 177)).toEqual([1, 2, 3, 4, 5, 'ellipsis-end', 177])
    expect(pageSlots(5, 177)).toEqual([1, 'ellipsis-start', 4, 5, 6, 'ellipsis-end', 177])
    expect(pageSlots(174, 177)).toEqual([1, 'ellipsis-start', 173, 174, 175, 176, 177])
    expect(pageSlots(177, 177)).toEqual([1, 'ellipsis-start', 173, 174, 175, 176, 177])
  })

  it('shows the same number of slots on every page', () => {
    for (let page = 1; page <= 40; page += 1) {
      expect(pageSlots(page, 40)).toHaveLength(7)
      expect(pageSlots(page, 40)).toContain(page)
    }
  })
})
