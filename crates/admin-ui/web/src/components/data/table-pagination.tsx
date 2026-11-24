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
import { pageSlots } from '@/lib/pagination'
import { cn } from '@/lib/utils'

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

  // Compared with the requested page, not the clamped one, so a link still works if the URL points
  // past the end. The route loaders redirect such a URL to the last page before it renders.
  function go(nextPage: number) {
    return (event: MouseEvent<HTMLAnchorElement>) => {
      event.preventDefault()
      if (nextPage !== page && nextPage >= 1 && nextPage <= pageCount) {
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
