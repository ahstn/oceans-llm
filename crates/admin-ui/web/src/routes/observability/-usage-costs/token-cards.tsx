import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'

import { BarListRow } from './bar-list'
import { formatCount, PERCENT_FORMATTER } from './shared'
import type { OwnerCacheRow } from './token-series'

export const CHART_SKELETON = <Skeleton className="h-72 w-full rounded-xl" />

/** Owner cache hit rates in the same row style as the owner spend breakdown. */
export function OwnerCacheList({ rows }: { rows: OwnerCacheRow[] }) {
  return (
    <ol data-testid="owner-cache-list" className="flex flex-col gap-1">
      {rows.map((row) => (
        <BarListRow
          key={row.key}
          label={row.name}
          detail={
            row.hitRate == null
              ? `${formatCount(row.inputTokens)} input · no cache split`
              : `${formatCount(row.cacheReadTokens)} of ${formatCount(row.inputTokens)} cached`
          }
          value={row.hitRate == null ? '—' : PERCENT_FORMATTER.format(row.hitRate)}
          progress={(row.hitRate ?? 0) * 100}
          progressLabel={`${row.name} cache hit rate`}
        />
      ))}
    </ol>
  )
}

export function NoTokens() {
  return (
    <Empty className="rounded-xl border bg-[color:var(--color-surface-muted)]">
      <EmptyHeader>
        <EmptyTitle>No token usage yet</EmptyTitle>
        <EmptyDescription>
          Token charts appear once ledger events with usage exist in the selected window.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  )
}
