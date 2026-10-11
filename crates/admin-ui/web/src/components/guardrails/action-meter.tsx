import { cn } from '@/lib/utils'

import { ACTION_FILL, ACTIONS, actionLabel } from './decision-meta'

/** One stacked bar of action shares; segments keep the action order so bars line up across rows. */
export function ActionMeter({
  counts,
  className,
}: {
  counts: Map<string, number>
  className?: string
}) {
  const total = [...counts.values()].reduce((sum, count) => sum + count, 0)
  const summary = ACTIONS.filter(({ id }) => counts.get(id))
    .map(({ id }) => `${counts.get(id)} ${actionLabel(id).toLowerCase()}`)
    .join(', ')
  return (
    <div
      role="img"
      aria-label={total ? summary : 'No decisions'}
      className={cn('bg-muted flex h-2 w-full overflow-hidden rounded-full', className)}
    >
      {ACTIONS.map(({ id }) => {
        const count = counts.get(id) ?? 0
        if (!count) return null
        return (
          <span
            key={id}
            className={ACTION_FILL[id]}
            style={{ width: `${(count / total) * 100}%` }}
          />
        )
      })}
    </div>
  )
}

export function ActionLegend() {
  return (
    <div className="text-muted-foreground flex flex-wrap items-center gap-3 text-xs">
      {ACTIONS.map(({ id, label }) => (
        <span key={id} className="flex items-center gap-1.5">
          <span aria-hidden className={cn('size-2 rounded-full', ACTION_FILL[id])} />
          {label}
        </span>
      ))}
    </div>
  )
}
