import type { ReactNode } from 'react'

import { CompactCount } from '@/components/data/compact-count'
import { Badge } from '@/components/ui/badge'
import { TableCell } from '@/components/ui/table'
import { EMPTY_VALUE, formatCount, formatTimestamp } from '@/lib/format'
import type { AttributionUserRefView } from '@/types/api'

import { formatAttributedCost, pullRequestLabel } from './shared'

const CELL = 'px-3 py-2'
const NUMERIC_CELL = 'px-3 py-2 text-right tabular-nums'
const TIMESTAMP_CELL = 'px-3 py-2 tabular-nums'

/** A text cell with the shared table padding. */
export function TextCell({ children }: { children: ReactNode }) {
  return <TableCell className={CELL}>{children}</TableCell>
}

export function CountCell({ value, unit }: { value: number; unit: string }) {
  return (
    <TableCell className={NUMERIC_CELL}>
      <CompactCount value={value} unit={unit} />
    </TableCell>
  )
}

export function TimestampCell({ value }: { value: string | null | undefined }) {
  return <TableCell className={TIMESTAMP_CELL}>{formatTimestamp(value)}</TableCell>
}

/** Requests without a price (unpriced or usage missing), worded as on Usage Costs. */
export function GapsBadge({ gaps }: { gaps: number }) {
  if (gaps <= 0) return null
  return (
    <Badge variant="warning">
      {formatCount(gaps)} {gaps === 1 ? 'gap' : 'gaps'}
    </Badge>
  )
}

/**
 * Priced cost, `—` when no request was priced, and a warning badge for the requests without a
 * price, as on Usage Costs.
 */
export function CostCell({
  cost,
  pricedRequests,
  gaps,
}: {
  cost: number
  pricedRequests: number
  /** Unpriced plus usage-missing requests. */
  gaps: number
}) {
  return (
    <TableCell className={NUMERIC_CELL}>
      <span className="inline-flex items-center justify-end gap-2">
        <GapsBadge gaps={gaps} />
        {formatAttributedCost(cost, pricedRequests)}
      </span>
    </TableCell>
  )
}

/** Model keys on one line, with the full list on hover. */
export function ModelsCell({ models, muted }: { models: string[]; muted?: boolean }) {
  return (
    <TableCell className={CELL}>
      {models.length === 0 ? (
        <span className="text-muted-foreground">{EMPTY_VALUE}</span>
      ) : (
        <span
          className={
            muted
              ? 'text-muted-foreground block max-w-56 truncate font-mono text-xs'
              : 'block max-w-56 truncate font-mono text-xs'
          }
          title={models.join(', ')}
        >
          {models.join(', ')}
        </span>
      )}
    </TableCell>
  )
}

/** An Oceans user's name, or `fallback` when no user matched. Attribution never carries emails. */
export function PersonCell({
  person,
  fallback,
}: {
  person: AttributionUserRefView | null | undefined
  fallback: ReactNode
}) {
  return (
    <TableCell className={CELL}>
      {person ? <span className="font-medium">{person.name}</span> : fallback}
    </TableCell>
  )
}

/** Only web links become anchors, so a reported URL can never run script. */
function safeWebUrl(value: string | null | undefined) {
  if (!value) return null
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null
  } catch {
    return null
  }
}

/** `repository#number`, linked to the pull request when it has a web URL. */
export function PullRequestLink({
  repository,
  number,
  url,
  label = pullRequestLabel(repository, number),
}: {
  repository: string
  number: number
  url: string | null | undefined
  label?: string
}) {
  const href = safeWebUrl(url)
  if (!href) return <span className="font-mono text-xs">{label}</span>
  return (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="font-mono text-xs underline underline-offset-4"
    >
      {label}
    </a>
  )
}
