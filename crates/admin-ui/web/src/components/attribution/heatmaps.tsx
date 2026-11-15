// Grid visuals on the heatmap tokens: a contribution calendar, a weekday by hour punch card, and a
// tool by model matrix. Each cell keeps its value in a native tooltip, so colour is never the only
// carrier of the number.

import type { CSSProperties, ReactNode } from 'react'

import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

const LEVELS = [
  'var(--heatmap-0)',
  'var(--heatmap-1)',
  'var(--heatmap-2)',
  'var(--heatmap-3)',
  'var(--heatmap-4)',
]

/** 0 for empty, then four active levels split by quartile of the busiest cell. */
function heatLevel(value: number, max: number) {
  if (value <= 0 || max <= 0) return 0
  return Math.min(4, 1 + Math.floor((value / max) * 3.999))
}

export function HeatLegend({ low = 'Less', high = 'More' }: { low?: string; high?: string }) {
  return (
    <div className="text-muted-foreground flex items-center gap-1.5 text-xs">
      {low}
      {LEVELS.map((tone) => (
        <span
          key={tone}
          className="size-2.5 rounded-xs bg-(--cell)"
          style={{ '--cell': tone } as CSSProperties}
        />
      ))}
      {high}
    </div>
  )
}

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/**
 * Days as columns of weeks, like a contribution graph. The cell shade is commits; a dot marks days
 * where AI wrote most of the attested lines.
 */
export function ContributionCalendar({
  cells,
}: {
  cells: { day: string; commits: number; aiShare: number | null; weekday: number }[]
}) {
  const max = Math.max(...cells.map((cell) => cell.commits))
  const weeks: (typeof cells)[] = []
  let week: typeof cells = []
  // Pad the first week so weekdays line up in rows.
  for (let pad = 0; pad < (cells[0]?.weekday ?? 0); pad += 1)
    week.push({ day: '', commits: -1, aiShare: null, weekday: pad })
  for (const cell of cells) {
    week.push(cell)
    if (cell.weekday === 6) {
      weeks.push(week)
      week = []
    }
  }
  if (week.length) weeks.push(week)
  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex gap-2 overflow-x-auto pb-1">
        <div className="text-muted-foreground text-2xs grid shrink-0 grid-rows-7 gap-1 pt-5">
          {WEEKDAYS.map((day, index) => (
            <span key={day} className="h-4 leading-4">
              {index % 2 === 1 ? day : ''}
            </span>
          ))}
        </div>
        <div className="flex gap-1">
          {weeks.map((days) => {
            const first = days.find((day) => day.day)
            const date = first ? new Date(first.day) : null
            const showMonth = date && date.getUTCDate() <= 7
            return (
              <div key={first?.day} className="flex flex-col gap-1">
                <span className="text-muted-foreground text-2xs h-4 whitespace-nowrap">
                  {showMonth && date ? MONTHS[date.getUTCMonth()] : ''}
                </span>
                {days.map((day) =>
                  day.commits < 0 ? (
                    <span key={`pad-${day.weekday}`} className="size-4" />
                  ) : (
                    <span
                      key={day.day}
                      title={`${day.day}: ${day.commits} commits${day.aiShare != null ? `, ${Math.round(day.aiShare * 100)}% AI lines` : ''}`}
                      className="relative grid size-4 place-items-center rounded-xs bg-(--cell)"
                      style={{ '--cell': LEVELS[heatLevel(day.commits, max)] } as CSSProperties}
                    >
                      {day.aiShare != null && day.aiShare >= 0.5 ? (
                        <span className="bg-background/80 size-1 rounded-full" />
                      ) : null}
                    </span>
                  ),
                )}
              </div>
            )
          })}
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <span className="text-muted-foreground inline-flex items-center gap-1.5 text-xs">
          <span className="bg-heatmap-3 grid size-4 place-items-center rounded-xs">
            <span className="bg-background/80 size-1 rounded-full" />
          </span>
          AI wrote most attested lines that day
        </span>
        <HeatLegend low="Fewer commits" high="More" />
      </div>
    </div>
  )
}

/** Monday first, Sunday last. */
const WEEKDAY_ORDER = [1, 2, 3, 4, 5, 6, 0]
const HOURS = Array.from({ length: 24 }, (_, hour) => hour)

/** Weekday by UTC hour. Circle size is commits; shade is the AI-majority share of them. */
export function PunchCard({ grid }: { grid: { ai: number; all: number }[][] }) {
  const max = Math.max(...grid.flat().map((cell) => cell.all))
  return (
    <div className="flex min-w-0 flex-col gap-2 overflow-x-auto">
      <div className="grid min-w-[40rem] grid-cols-[2.5rem_repeat(24,minmax(0,1fr))] items-center gap-y-1">
        {WEEKDAY_ORDER.map((weekday) => (
          <PunchRow key={weekday} label={WEEKDAYS[weekday]}>
            {HOURS.map((hour) => {
              const cell = grid[weekday][hour]
              const size = cell.all ? 4 + (cell.all / max) * 14 : 0
              const share = cell.all ? cell.ai / cell.all : 0
              return (
                <span
                  key={hour}
                  className="grid h-5 place-items-center"
                  title={`${WEEKDAYS[weekday]} ${hour}:00 UTC: ${cell.all} commits, ${Math.round(share * 100)}% mostly AI`}
                >
                  {cell.all ? (
                    <span
                      className="rounded-full bg-(--dot)"
                      style={
                        {
                          '--dot': LEVELS[1 + Math.min(3, Math.floor(share * 4))],
                          width: size,
                          height: size,
                        } as CSSProperties
                      }
                    />
                  ) : null}
                </span>
              )
            })}
          </PunchRow>
        ))}
        <span />
        {HOURS.map((hour) => (
          <span key={hour} className="text-muted-foreground text-2xs text-center tabular-nums">
            {hour % 3 === 0 ? hour : ''}
          </span>
        ))}
      </div>
    </div>
  )
}

function PunchRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <span className="text-muted-foreground text-xs">{label}</span>
      {children}
    </>
  )
}

/** Rows by columns with a shaded value per cell, for tool by model comparisons. */
export function MatrixHeatmap<Row extends string, Column extends string>({
  rows,
  columns,
  value,
  format,
  rowLabel,
  columnLabel,
  invert = false,
  caption,
}: {
  rows: Row[]
  columns: Column[]
  value: (row: Row, column: Column) => number | null
  format: (value: number) => string
  rowLabel: (row: Row) => ReactNode
  columnLabel: (column: Column) => string
  /** Shade low values brightest, for costs where lower is better. */
  invert?: boolean
  caption: string
}) {
  const values: number[] = []
  for (const row of rows) {
    for (const column of columns) {
      const entry = value(row, column)
      if (entry != null) values.push(entry)
    }
  }
  const min = Math.min(...values)
  const max = Math.max(...values)
  return (
    <div className="border-border overflow-x-auto rounded-md border">
      <Table aria-label={caption} className="min-w-[40rem]">
        <AdminTableHeader>
          <AdminTableHead>Harness</AdminTableHead>
          {columns.map((column) => (
            <AdminTableHead key={column} className="px-2 font-mono text-xs font-normal">
              {columnLabel(column)}
            </AdminTableHead>
          ))}
        </AdminTableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={row}>
              <TableCell className="px-3 py-2">{rowLabel(row)}</TableCell>
              {columns.map((column) => {
                const cell = value(row, column)
                const position = cell == null || max === min ? 0.5 : (cell - min) / (max - min)
                const level =
                  cell == null
                    ? 0
                    : 1 + Math.min(3, Math.floor((invert ? 1 - position : position) * 4))
                return (
                  <TableCell key={column} className="p-1">
                    <span
                      className={cn(
                        'flex h-9 items-center justify-end rounded-md px-2 text-xs tabular-nums',
                        cell == null
                          ? 'text-muted-foreground'
                          : level >= 3
                            ? 'text-background font-medium'
                            : 'text-foreground',
                      )}
                      style={{ background: cell == null ? 'transparent' : LEVELS[level] }}
                    >
                      {cell == null ? '—' : format(cell)}
                    </span>
                  </TableCell>
                )
              })}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
