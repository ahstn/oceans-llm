// Building blocks of the Attribution tabs and their concept mockups: the data-readiness badge, the
// section card, KPI tiles with sparklines, the AI / human split bar and harness labels.

import { ArrowDownRight01Icon, ArrowUpRight01Icon } from '@hugeicons/core-free-icons'
import { useId, type CSSProperties, type ReactNode } from 'react'

import { AgentHarnessIcon } from '@/components/icons/agent-harness-icon'
import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

import { formatPercent, HARNESSES, READINESS, SIGNED_PERCENT, type Readiness } from './format'

// Attribution data roles. AI and human are the two fixed series; their lightness differs (blue
// against a lighter green) so a stack of them reads without colour. Unattested lines are unknown,
// not human, and take the grouped "other" colour.
export const AI_TONE = 'var(--chart-1)'
export const HUMAN_TONE = 'var(--chart-2)'
export const UNKNOWN_TONE = 'var(--chart-other)'
/** Spend that shipped vs spend that did not. */
export const SHIPPED_TONE = 'var(--chart-4)'
export const UNSHIPPED_TONE = 'var(--chart-other)'

// Data readiness -----------------------------------------------------------------------------------

export function ReadinessBadge({ level, note }: { level: Readiness; note?: string }) {
  const meta = READINESS[level]
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant={meta.variant} tabIndex={0} data-readiness={level}>
          {meta.label}
        </Badge>
      </TooltipTrigger>
      <TooltipContent className="max-w-72">
        {meta.detail}
        {note ? ` ${note}` : null}
      </TooltipContent>
    </Tooltip>
  )
}

/** A report section, with a readiness badge in the header when its data is a proxy or missing. */
export function InsightCard({
  title,
  description,
  readiness,
  readinessNote,
  action,
  children,
  className,
  contentClassName,
}: {
  title: string
  description?: ReactNode
  /** Omit for data Oceans already has; set to flag a proxy or a gap. */
  readiness?: Readiness
  readinessNote?: string
  action?: ReactNode
  children: ReactNode
  className?: string
  contentClassName?: string
}) {
  return (
    <Card className={cn('min-w-0', className)}>
      <CardHeader>
        <CardTitle className="flex flex-wrap items-center gap-2">
          {title}
          {readiness ? <ReadinessBadge level={readiness} note={readinessNote} /> : null}
        </CardTitle>
        {description ? <CardDescription>{description}</CardDescription> : null}
        {action ? <CardAction>{action}</CardAction> : null}
      </CardHeader>
      <CardContent className={cn('min-w-0', contentClassName)}>{children}</CardContent>
    </Card>
  )
}

// Sparkline ----------------------------------------------------------------------------------------

/** A compact trend line. Gaps (`null`) break the line rather than dropping to zero. */
export function Sparkline({
  values,
  tone = 'var(--chart-hero)',
  height = 36,
  area = true,
  label,
  className,
}: {
  values: (number | null)[]
  tone?: string
  height?: number
  area?: boolean
  label?: string
  className?: string
}) {
  const gradientId = useId().replace(/:/g, '')
  const finite = values.filter((value): value is number => value != null)
  if (finite.length < 2) return <div style={{ height }} className={className} />
  const min = Math.min(...finite)
  const max = Math.max(...finite)
  const span = max - min || 1
  const width = 100
  const step = width / (values.length - 1)
  const points = values.map((value, index) =>
    value == null ? null : ([index * step, 2 + (1 - (value - min) / span) * (height - 4)] as const),
  )
  let line = ''
  let pen = false
  for (const point of points) {
    if (!point) {
      pen = false
      continue
    }
    line += `${pen ? 'L' : 'M'}${point[0].toFixed(2)},${point[1].toFixed(2)}`
    pen = true
  }
  const first = points.find(Boolean)
  const last = [...points].reverse().find(Boolean)
  const fill =
    area && first && last ? `${line}L${last[0]},${height}L${first[0]},${height}Z` : undefined
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className={cn('block w-full overflow-visible', className)}
      style={{ height }}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0%" stopColor={tone} stopOpacity={0.28} />
          <stop offset="100%" stopColor={tone} stopOpacity={0} />
        </linearGradient>
      </defs>
      {fill ? <path d={fill} fill={`url(#${gradientId})`} /> : null}
      <path
        d={line}
        fill="none"
        stroke={tone}
        strokeWidth={1.5}
        vectorEffect="non-scaling-stroke"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
      {last ? (
        <circle cx={last[0]} cy={last[1]} r={2.2} fill={tone} vectorEffect="non-scaling-stroke" />
      ) : null}
    </svg>
  )
}

// KPI tiles ----------------------------------------------------------------------------------------

/**
 * Change against the previous window. Deltas stay neutral ink: whether up is good depends on the
 * metric, and green, amber and rose are reserved for state.
 */
export function DeltaChip({
  value,
  suffix = 'vs previous',
}: {
  value: number | null
  suffix?: string
}) {
  if (value == null) return <span className="text-muted-foreground text-xs">No prior period</span>
  const up = value >= 0
  return (
    <span className="text-muted-foreground inline-flex items-center gap-1 text-xs tabular-nums">
      <span className="text-foreground inline-flex items-center gap-0.5 font-medium">
        <AppIcon
          icon={up ? ArrowUpRight01Icon : ArrowDownRight01Icon}
          size={14}
          stroke={1.5}
          aria-hidden
        />
        {SIGNED_PERCENT.format(value)}
      </span>
      {suffix}
    </span>
  )
}

export function TrendKpi({
  label,
  value,
  unit,
  delta,
  hint,
  spark,
  tone,
  readiness,
  readinessNote,
}: {
  label: string
  value: string
  unit?: string
  delta?: number | null
  hint?: ReactNode
  spark?: (number | null)[]
  tone?: string
  /** Omit for data Oceans already has; set to flag a proxy or a gap. */
  readiness?: Readiness
  readinessNote?: string
}) {
  return (
    <Card size="sm" className="min-w-0">
      <CardHeader>
        <CardDescription className="flex items-start justify-between gap-2">
          <span className="min-w-0">{label}</span>
          {readiness ? (
            <span className="shrink-0">
              <ReadinessBadge level={readiness} note={readinessNote} />
            </span>
          ) : null}
        </CardDescription>
        <p className="flex items-baseline gap-1.5 text-2xl font-medium tracking-tight tabular-nums">
          {value}
          {unit ? <span className="text-muted-foreground text-sm font-normal">{unit}</span> : null}
        </p>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {spark ? <Sparkline values={spark} tone={tone} height={32} /> : null}
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
          {delta !== undefined ? <DeltaChip value={delta} /> : null}
          {hint ? <span className="text-muted-foreground text-xs">{hint}</span> : null}
        </div>
      </CardContent>
    </Card>
  )
}

// Split bars and meters ----------------------------------------------------------------------------

export function Swatch({ tone, label, value }: { tone: string; label: string; value?: string }) {
  return (
    <span className="text-muted-foreground inline-flex items-center gap-1.5 text-xs">
      <span
        className="size-2 shrink-0 rounded-full bg-(--swatch)"
        style={{ '--swatch': tone } as CSSProperties}
        aria-hidden
      />
      {label}
      {value ? <span className="text-foreground tabular-nums">{value}</span> : null}
    </span>
  )
}

/** AI, human and unattested lines as one bar. Unattested lines are unknown, never human. */
export function AiShareBar({
  ai,
  human,
  unknown = 0,
  legend = false,
  size = 'default',
}: {
  ai: number
  human: number
  unknown?: number
  legend?: boolean
  size?: 'default' | 'sm'
}) {
  const total = ai + human + unknown
  const segments = [
    { key: 'ai', tone: AI_TONE, value: ai, label: 'AI' },
    { key: 'human', tone: HUMAN_TONE, value: human, label: 'Human' },
    { key: 'unknown', tone: UNKNOWN_TONE, value: unknown, label: 'Unattested' },
  ].filter((segment) => segment.value > 0)
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div
        className={cn(
          'bg-muted flex w-full gap-px overflow-hidden rounded-full',
          size === 'sm' ? 'h-1.5' : 'h-2.5',
        )}
        role="img"
        aria-label={`${formatPercent(total ? ai / total : null)} AI, ${formatPercent(total ? human / total : null)} human`}
      >
        {segments.map((segment) => (
          <span
            key={segment.key}
            className="h-full bg-(--segment)"
            style={
              {
                '--segment': segment.tone,
                width: `${(segment.value / total) * 100}%`,
              } as CSSProperties
            }
          />
        ))}
      </div>
      {legend ? (
        <div className="flex flex-wrap gap-x-3 gap-y-1">
          {segments.map((segment) => (
            <Swatch
              key={segment.key}
              tone={segment.tone}
              label={segment.label}
              value={formatPercent(segment.value / total)}
            />
          ))}
        </div>
      ) : null}
    </div>
  )
}

/** A thin 0–1 meter for table cells, with its value beside it. */
export function InlineMeter({
  value,
  tone = 'var(--chart-hero)',
  label,
  className,
}: {
  value: number | null
  tone?: string
  label?: string
  className?: string
}) {
  return (
    <div className={cn('flex min-w-28 items-center gap-2', className)}>
      <div
        className="h-1.5 flex-1 overflow-hidden rounded-full bg-(--meter)/18"
        style={{ '--meter': tone } as CSSProperties}
      >
        <div
          className="h-full rounded-full bg-(--meter)"
          style={{ width: `${Math.max(0, Math.min(1, value ?? 0)) * 100}%` }}
        />
      </div>
      <span className="w-10 text-right text-xs tabular-nums">{label ?? formatPercent(value)}</span>
    </div>
  )
}

/** A small labelled figure for stat strips inside a card. */
export function Stat({
  label,
  value,
  hint,
}: {
  label: string
  value: ReactNode
  hint?: ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="text-muted-foreground text-xs">{label}</span>
      <span className="text-lg font-medium tabular-nums">{value}</span>
      {hint ? <span className="text-muted-foreground text-xs">{hint}</span> : null}
    </div>
  )
}

/** A one-line takeaway under a chart: what the reader should notice. */
export function Insight({ children }: { children: ReactNode }) {
  return (
    <p className="text-muted-foreground border-border border-t pt-3 text-xs">
      <span className="text-foreground font-medium">Read: </span>
      {children}
    </p>
  )
}

/** A harness mark and name. Harnesses without a brand mark get their initial in a tile. */
export function ToolLabel({ harness, hideName = false }: { harness: string; hideName?: boolean }) {
  const known = HARNESSES[harness]
  const label = known?.label ?? harness
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5" title={label}>
      {known?.icon ? (
        <AgentHarnessIcon harnessKey={known.icon} size={14} className="text-foreground" />
      ) : (
        <span
          className="bg-muted text-foreground text-2xs grid size-3.5 place-items-center rounded-sm font-semibold uppercase"
          aria-hidden
        >
          {label[0]}
        </span>
      )}
      {hideName ? (
        <span className="sr-only">{label}</span>
      ) : (
        <span className="truncate">{label}</span>
      )}
    </span>
  )
}

export type PairSide = { label: string; value: string; delta: number | null; readiness?: Readiness }

/** Two metrics that pull against each other, read together. */
export function CounterweightPair({ left, right }: { left: PairSide; right: PairSide }) {
  return (
    <div className="border-border grid grid-cols-[1fr_auto_1fr] items-stretch rounded-lg border">
      <PairCell side={left} />
      <div
        className="border-border text-muted-foreground flex items-center border-x px-2 text-xs"
        aria-hidden
      >
        vs
      </div>
      <PairCell side={right} />
    </div>
  )
}

function PairCell({ side }: { side: PairSide }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 p-3">
      <span className="text-muted-foreground text-xs">{side.label}</span>
      <span className="text-lg font-medium tabular-nums">{side.value}</span>
      <span className="flex flex-wrap items-center gap-2 text-xs">
        {side.readiness ? <ReadinessBadge level={side.readiness} /> : null}
        {side.delta != null ? (
          <span className="text-muted-foreground tabular-nums">
            {side.delta >= 0 ? '+' : ''}
            {Math.round(side.delta * 100)}% vs previous
          </span>
        ) : null}
      </span>
    </div>
  )
}
