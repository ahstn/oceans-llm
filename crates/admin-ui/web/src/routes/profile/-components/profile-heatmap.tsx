import { useMemo, useRef, useState, type CSSProperties, type PointerEvent } from 'react'

import { formatUsd10000 } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { MyProfileView } from '@/types/api'

import {
  buildHeatmap,
  COMPACT_FORMATTER,
  formatLongDay,
  NUMBER_FORMATTER,
  type HeatmapCell,
} from './profile-data'

/** Primary blue at rising strength reads the same way in light and dark themes. */
const LEVEL_BACKGROUND: Record<HeatmapCell['level'], string> = {
  0: 'var(--color-surface-muted)',
  1: 'color-mix(in oklch, var(--primary) 28%, transparent)',
  2: 'color-mix(in oklch, var(--primary) 50%, transparent)',
  3: 'color-mix(in oklch, var(--primary) 75%, transparent)',
  4: 'var(--primary)',
}

/** Sunday-first rows; alternate labels keep the axis readable at small cell sizes. */
const WEEKDAYS = [
  { day: 'Sun', show: false },
  { day: 'Mon', show: true },
  { day: 'Tue', show: false },
  { day: 'Wed', show: true },
  { day: 'Thu', show: false },
  { day: 'Fri', show: true },
  { day: 'Sat', show: false },
]

/** Tooltip anchor relative to the heatmap wrapper; `below` flips it under top-row cells. */
type HoverState = { cell: HeatmapCell; left: number; top: number; below: boolean }

/** Half the tooltip's minimum width, so it stays inside the card at either edge. */
const TOOLTIP_HALF_WIDTH = 96

function describeCell(cell: HeatmapCell) {
  if (!cell.usage || cell.usage.request_count === 0)
    return `${formatLongDay(cell.day)}: no activity`
  return `${formatLongDay(cell.day)}: ${NUMBER_FORMATTER.format(cell.usage.request_count)} requests, ${COMPACT_FORMATTER.format(cell.usage.total_tokens)} tokens, ${formatUsd10000(cell.usage.cost_usd_10000)}`
}

/**
 * Calendar of daily token volume. One shared tooltip follows the pointer so a year of cells
 * does not mount a year of tooltip instances.
 */
export function UsageHeatmap({
  profile,
  weeks = 53,
  className,
}: {
  profile: MyProfileView
  weeks?: number
  className?: string
}) {
  const heatmap = useMemo(() => buildHeatmap(profile, weeks), [profile, weeks])
  const wrapperRef = useRef<HTMLDivElement>(null)
  const [hover, setHover] = useState<HoverState | null>(null)
  const cells = useMemo(
    () => heatmap.weeks.flat().filter((cell): cell is HeatmapCell => cell !== null),
    [heatmap],
  )
  const cellsByDay = useMemo(() => new Map(cells.map((cell) => [cell.day, cell])), [cells])

  function handlePointerOver(event: PointerEvent<HTMLDivElement>) {
    const target = (event.target as HTMLElement).closest<HTMLElement>('[data-day]')
    const cell = target ? cellsByDay.get(target.dataset.day ?? '') : undefined
    const wrapper = wrapperRef.current
    if (!target || !cell || !wrapper) return
    const wrapperBox = wrapper.getBoundingClientRect()
    const cellBox = target.getBoundingClientRect()
    const center = cellBox.left - wrapperBox.left + cellBox.width / 2
    // Sunday to Tuesday rows sit near the card header, so their tooltip opens downwards.
    const below = new Date(`${cell.day}T00:00:00Z`).getUTCDay() < 3
    setHover({
      cell,
      left: Math.min(Math.max(center, TOOLTIP_HALF_WIDTH), wrapperBox.width - TOOLTIP_HALF_WIDTH),
      top: below ? cellBox.bottom - wrapperBox.top + 6 : cellBox.top - wrapperBox.top - 6,
      below,
    })
  }

  const columns = { gridTemplateColumns: `repeat(${weeks}, minmax(0, 1fr))` } as CSSProperties

  return (
    <div ref={wrapperRef} className={cn('relative flex flex-col gap-2', className)}>
      <div className="overflow-x-auto">
        <div className="flex min-w-[36rem] gap-2">
          <div className="text-muted-foreground mt-5 grid grid-rows-7 gap-[3px] text-[10px] leading-none">
            {WEEKDAYS.map(({ day, show }) => (
              <span key={day} className="flex items-center">
                {show ? day : ''}
              </span>
            ))}
          </div>
          <div className="relative flex min-w-0 flex-1 flex-col gap-1.5">
            <div className="text-muted-foreground grid h-3.5 text-[10px]" style={columns}>
              {heatmap.months.map((month) => (
                <span
                  key={`${month.label}-${month.week}`}
                  className="whitespace-nowrap"
                  style={{ gridColumnStart: month.week + 1 }}
                >
                  {month.label}
                </span>
              ))}
            </div>
            <div
              data-testid="usage-heatmap"
              role="group"
              aria-label="Daily token usage"
              className="relative grid grid-flow-col grid-rows-7 gap-[3px]"
              style={columns}
              onPointerOver={handlePointerOver}
              onPointerLeave={() => setHover(null)}
            >
              {/* Only days after today are null, so trailing cells can be skipped in column flow. */}
              {cells.map((cell) => (
                <div
                  key={cell.day}
                  role="img"
                  data-day={cell.day}
                  data-level={cell.level}
                  aria-label={describeCell(cell)}
                  className="aspect-square rounded-[2px] outline-offset-1 hover:outline hover:outline-1 hover:outline-[var(--foreground)]"
                  style={{ background: LEVEL_BACKGROUND[cell.level] }}
                />
              ))}
            </div>
          </div>
        </div>
      </div>
      {/* Outside the scroll container so the tooltip is never clipped by it. */}
      {hover ? <HeatmapTooltip hover={hover} /> : null}
      <HeatmapLegend />
    </div>
  )
}

function HeatmapTooltip({ hover }: { hover: HoverState }) {
  const usage = hover.cell.usage
  return (
    <div
      role="tooltip"
      data-testid="heatmap-tooltip"
      className={cn(
        'bg-popover text-popover-foreground pointer-events-none absolute z-10 w-max min-w-44 -translate-x-1/2 rounded-md border px-3 py-2 text-xs shadow-md',
        !hover.below && '-translate-y-full',
      )}
      style={{ left: hover.left, top: hover.top }}
    >
      <p className="mb-1.5 font-medium">{formatLongDay(hover.cell.day)}</p>
      {usage && usage.request_count > 0 ? (
        <dl className="grid grid-cols-[auto_auto] gap-x-4 gap-y-0.5">
          <dt className="text-muted-foreground">Requests</dt>
          <dd className="text-right tabular-nums">
            {NUMBER_FORMATTER.format(usage.request_count)}
          </dd>
          <dt className="text-muted-foreground">Total tokens</dt>
          <dd className="text-right tabular-nums">{NUMBER_FORMATTER.format(usage.total_tokens)}</dd>
          <dt className="text-muted-foreground">Cost</dt>
          <dd className="text-right tabular-nums">{formatUsd10000(usage.cost_usd_10000)}</dd>
        </dl>
      ) : (
        <p className="text-muted-foreground">No activity</p>
      )}
    </div>
  )
}

function HeatmapLegend() {
  return (
    <div className="text-muted-foreground flex items-center justify-end gap-1 text-[10px]">
      <span className="mr-1">Fewer tokens</span>
      {([0, 1, 2, 3, 4] as const).map((level) => (
        <span
          key={level}
          aria-hidden
          className="size-2.5 rounded-[2px]"
          style={{ background: LEVEL_BACKGROUND[level] }}
        />
      ))}
      <span className="ml-1">More</span>
    </div>
  )
}
