import type { MouseEvent } from 'react'

import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from '@/components/ui/pagination'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { formatCount } from '@/lib/format'
import { cn } from '@/lib/utils'

/** Page buttons shown at most, counting the first and last page and each ellipsis. */
const pageSlotCount = 7

interface TablePaginationProps {
  total: number
  page: number
  pageSize: number
  pageSizes: readonly number[]
  /** Dims the pager while the next page loads. */
  pending?: boolean
  onPageChange: (page: number, pageSize: number) => void
  className?: string
}

/**
 * A server-paged table footer, after ReUI's `c-pagination-15`: the shown range on the left, then
 * Previous, numbered pages with an ellipsis for each hidden stretch, Next, and rows per page. The
 * first and last page stay one click away, and the page row keeps one width as you page.
 */
export function TablePagination({
  total,
  page,
  pageSize,
  pageSizes,
  pending = false,
  onPageChange,
  className,
}: TablePaginationProps) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  const currentPage = Math.min(Math.max(page, 1), pageCount)
  const first = total === 0 ? 0 : (currentPage - 1) * pageSize + 1
  const last = Math.min(currentPage * pageSize, total)

  function go(nextPage: number) {
    return (event: MouseEvent<HTMLAnchorElement>) => {
      event.preventDefault()
      if (nextPage !== currentPage && nextPage >= 1 && nextPage <= pageCount) {
        onPageChange(nextPage, pageSize)
      }
    }
  }

  return (
    <div
      className={cn(
        'flex min-w-0 flex-wrap items-center justify-between gap-3 text-sm',
        pending && 'opacity-60 transition-opacity',
        className,
      )}
      aria-busy={pending}
      data-testid="table-pagination"
    >
      <span className="text-muted-foreground tabular-nums">
        {formatCount(first)}–{formatCount(last)} of {formatCount(total)}
      </span>
      <div className="flex flex-wrap items-center gap-3">
        <Pagination className="mx-0 w-auto">
          <PaginationContent>
            <PaginationItem>
              <PaginationPrevious
                href="#"
                aria-disabled={currentPage <= 1}
                tabIndex={currentPage <= 1 ? -1 : undefined}
                className={cn(currentPage <= 1 && 'pointer-events-none opacity-50')}
                onClick={go(currentPage - 1)}
              />
            </PaginationItem>
            {pageSlots(currentPage, pageCount).map((slot) =>
              typeof slot === 'number' ? (
                <PaginationItem key={slot}>
                  <PaginationLink
                    href="#"
                    isActive={slot === currentPage}
                    aria-label={`Go to page ${slot}`}
                    className="tabular-nums"
                    onClick={go(slot)}
                  >
                    {slot}
                  </PaginationLink>
                </PaginationItem>
              ) : (
                <PaginationItem key={slot}>
                  <PaginationEllipsis />
                </PaginationItem>
              ),
            )}
            <PaginationItem>
              <PaginationNext
                href="#"
                aria-disabled={currentPage >= pageCount}
                tabIndex={currentPage >= pageCount ? -1 : undefined}
                className={cn(currentPage >= pageCount && 'pointer-events-none opacity-50')}
                onClick={go(currentPage + 1)}
              />
            </PaginationItem>
          </PaginationContent>
        </Pagination>
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground hidden sm:inline">Rows per page</span>
          <Select
            value={String(pageSize)}
            onValueChange={(value) => onPageChange(1, Number(value))}
          >
            <SelectTrigger size="sm" aria-label="Rows per page" className="tabular-nums">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {pageSizes.map((size) => (
                <SelectItem key={size} value={String(size)}>
                  {size}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
    </div>
  )
}

type PageSlot = number | 'ellipsis-start' | 'ellipsis-end'

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
