import type { CSSProperties, ReactNode } from 'react'

import { Progress } from '@/components/ui/progress'
import { Skeleton } from '@/components/ui/skeleton'

/** Ranked list rows shared by the breakdown cards: label and badge left, detail and value right, bar below. */
export function BarListRow({
  label,
  mono,
  badge,
  detail,
  value,
  progress,
  progressLabel,
  tone,
}: {
  label: string
  mono?: boolean
  badge?: ReactNode
  detail: string
  value: string
  /** 0–100. */
  progress: number
  /** Announces the bar when it carries data not repeated in the text. */
  progressLabel?: string
  /** CSS colour for the bar; the track takes a faint wash of it. Defaults to primary. */
  tone?: string
}) {
  return (
    <li className="flex flex-col gap-1.5 py-2">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className={mono ? 'truncate font-mono text-sm' : 'truncate text-sm'}>{label}</span>
          {badge}
        </div>
        <div className="flex shrink-0 items-baseline gap-2">
          <span className="text-muted-foreground text-xs tabular-nums">{detail}</span>
          <span className="text-sm font-medium tabular-nums">{value}</span>
        </div>
      </div>
      <Progress
        value={progress}
        className={
          tone
            ? 'bg-[color-mix(in_oklch,var(--bar-tone)_18%,transparent)] [&>[data-slot=progress-indicator]]:bg-[var(--bar-tone)]'
            : undefined
        }
        style={tone ? ({ '--bar-tone': tone } as CSSProperties) : undefined}
        {...(progressLabel ? { 'aria-label': progressLabel } : { 'aria-hidden': true })}
      />
    </li>
  )
}

export function BarListSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      {['a', 'b', 'c', 'd'].map((row) => (
        <Skeleton key={row} className="h-9 w-full rounded-md" />
      ))}
    </div>
  )
}

export function BarListEmpty({ children }: { children: ReactNode }) {
  return <p className="text-muted-foreground py-6 text-center text-sm">{children}</p>
}
