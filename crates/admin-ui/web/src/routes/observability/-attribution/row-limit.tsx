import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { formatCount } from '@/lib/format'

/** Rows a long attribution table shows at first and adds per "Show more". */
const ROW_STEP = 50

/** The first rows of a long list, so a 1,000-commit window doesn't render every row at once. */
export function useRowLimit<T>(rows: T[]) {
  const [limit, setLimit] = useState(ROW_STEP)
  return {
    visible: rows.slice(0, limit),
    showMore: () => setLimit((current) => current + ROW_STEP),
  }
}

export function RowLimitFooter({
  shown,
  total,
  noun,
  onShowMore,
}: {
  shown: number
  total: number
  /** Plural noun for the count, e.g. `changes`. */
  noun: string
  onShowMore: () => void
}) {
  return (
    <div className="mt-3 flex flex-wrap items-center justify-between gap-3 text-sm">
      <span className="text-muted-foreground tabular-nums">
        Showing {formatCount(shown)} of {formatCount(total)} {noun}
      </span>
      {shown < total ? (
        <Button type="button" variant="outline" size="sm" onClick={onShowMore}>
          Show more
        </Button>
      ) : null}
    </div>
  )
}
