/** Page buttons shown at most, counting the first and last page and each ellipsis. */
const pageSlotCount = 7

export type PageSlot = number | 'ellipsis-start' | 'ellipsis-end'

/**
 * Which page buttons to show: every page when they fit in `pageSlotCount`, otherwise the first
 * page, a run around the current one, and the last page, with an ellipsis for each gap. The slot
 * count stays the same on every page, so the row never changes width.
 */
export function pageSlots(page: number, pageCount: number): PageSlot[] {
  if (pageCount <= pageSlotCount) {
    return Array.from({ length: pageCount }, (_, index) => index + 1)
  }
  // Slots left for the run once the first and last page and both ellipses are placed.
  const middleSlots = pageSlotCount - 4
  const runStart = Math.min(
    Math.max(page - Math.floor(middleSlots / 2), 3),
    pageCount - middleSlots - 1,
  )
  // Near either end the ellipsis on that side is not needed, so the run takes its slot.
  if (runStart <= 3) {
    return [...range(1, pageSlotCount - 2), 'ellipsis-end', pageCount]
  }
  if (runStart + middleSlots - 1 >= pageCount - 2) {
    return [1, 'ellipsis-start', ...range(pageCount - (pageSlotCount - 3), pageCount)]
  }
  return [
    1,
    'ellipsis-start',
    ...range(runStart, runStart + middleSlots - 1),
    'ellipsis-end',
    pageCount,
  ]
}

function range(start: number, end: number) {
  return Array.from({ length: end - start + 1 }, (_, index) => start + index)
}

/**
 * The last page when `page` is past it, so a route loader can redirect there; otherwise `null`. A
 * saved URL can point past the end, for example after retention removed rows.
 */
export function pageBeyondLast(page: number | undefined, pageSize: number, total: number) {
  const lastPage = Math.max(1, Math.ceil(total / pageSize))
  return (page ?? 1) > lastPage ? lastPage : null
}
