import { describe, expect, it } from 'vitest'

import { pageBeyondLast, pageSlots } from '@/lib/pagination'

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

describe('pageBeyondLast', () => {
  it('returns the last page only for a page past the end', () => {
    expect(pageBeyondLast(99, 100, 1_051)).toBe(11)
    expect(pageBeyondLast(9, 25, 44)).toBe(2)
    expect(pageBeyondLast(11, 100, 1_051)).toBeNull()
    expect(pageBeyondLast(undefined, 100, 1_051)).toBeNull()
  })

  it('sends any later page of an empty list to page 1', () => {
    expect(pageBeyondLast(3, 50, 0)).toBe(1)
    expect(pageBeyondLast(1, 50, 0)).toBeNull()
  })
})
