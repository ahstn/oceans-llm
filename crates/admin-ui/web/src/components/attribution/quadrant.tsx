// A two-axis quadrant in SVG: one bubble per item, sized by a third value, with dividers at the
// weighted average of each axis so quadrants read as above or below the group, not against fixed
// targets. Axes zoom to the data.

import type { CSSProperties } from 'react'

import { cn } from '@/lib/utils'

import { formatPercent } from './parts'

export type QuadrantPoint = {
  key: string
  label: string
  x: number
  y: number
  /** Bubble area. */
  size: number
  /** Weight for the average dividers. */
  weight: number
  title: string
}

// Scales, dividers, labels and bubbles share one coordinate system, so they stay in one SVG.
// oxlint-disable-next-line eslint/max-lines-per-function
export function Quadrant({
  points,
  xLabel,
  yLabel,
  labels,
  focus,
  onFocus,
  ariaLabel,
}: {
  points: QuadrantPoint[]
  xLabel: string
  yLabel: string
  /** Top left, top right, bottom left, bottom right. */
  labels: [string, string, string, string]
  focus?: string | null
  onFocus?: (key: string | null) => void
  ariaLabel: string
}) {
  const size = 360
  const pad = 40
  const domain = (values: number[]) => [
    Math.max(0, Math.floor((Math.min(...values) - 0.06) * 10) / 10),
    Math.min(1, Math.ceil((Math.max(...values) + 0.06) * 10) / 10),
  ]
  const [x0, x1] = points.length ? domain(points.map((point) => point.x)) : [0, 1]
  const [y0, y1] = points.length ? domain(points.map((point) => point.y)) : [0, 1]
  const weight = Math.max(
    1,
    points.reduce((total, point) => total + point.weight, 0),
  )
  const meanX = points.reduce((total, point) => total + point.x * point.weight, 0) / weight
  const meanY = points.reduce((total, point) => total + point.y * point.weight, 0) / weight
  const maxSize = Math.max(1, ...points.map((point) => point.size))
  const x = (value: number) => pad + ((value - x0) / Math.max(0.01, x1 - x0)) * (size - pad * 2)
  const y = (value: number) =>
    size - pad - ((value - y0) / Math.max(0.01, y1 - y0)) * (size - pad * 2)
  const corners = [
    { label: labels[0], x: pad + 6, y: pad + 14, end: false },
    { label: labels[1], x: size - pad - 6, y: pad + 14, end: true },
    { label: labels[2], x: pad + 6, y: size - pad - 8, end: false },
    { label: labels[3], x: size - pad - 6, y: size - pad - 8, end: true },
  ]
  return (
    <svg
      viewBox={`0 0 ${size} ${size}`}
      className="h-auto w-full"
      role="img"
      aria-label={ariaLabel}
    >
      <rect
        x={pad}
        y={pad}
        width={size - pad * 2}
        height={size - pad * 2}
        className="stroke-border fill-none"
      />
      {points.length ? (
        <>
          <line
            x1={x(meanX)}
            x2={x(meanX)}
            y1={pad}
            y2={size - pad}
            className="stroke-border"
            strokeDasharray="4 3"
          />
          <line
            x1={pad}
            x2={size - pad}
            y1={y(meanY)}
            y2={y(meanY)}
            className="stroke-border"
            strokeDasharray="4 3"
          />
        </>
      ) : null}
      {corners.map((corner) => (
        <text
          key={corner.label}
          x={corner.x}
          y={corner.y}
          textAnchor={corner.end ? 'end' : 'start'}
          className="fill-muted-foreground text-2xs"
        >
          {corner.label}
        </text>
      ))}
      {[x0, x1].map((tick) => (
        <text
          key={`x${tick}`}
          x={x(tick)}
          y={size - pad + 14}
          textAnchor="middle"
          className="fill-muted-foreground text-2xs"
        >
          {formatPercent(tick)}
        </text>
      ))}
      {[y0, y1].map((tick) => (
        <text
          key={`y${tick}`}
          x={pad - 6}
          y={y(tick) + 3}
          textAnchor="end"
          className="fill-muted-foreground text-2xs"
        >
          {formatPercent(tick)}
        </text>
      ))}
      <text
        x={size / 2}
        y={size - 6}
        textAnchor="middle"
        className="fill-muted-foreground text-2xs"
      >
        {xLabel}
      </text>
      <text
        x={12}
        y={size / 2}
        textAnchor="middle"
        transform={`rotate(-90 12 ${size / 2})`}
        className="fill-muted-foreground text-2xs"
      >
        {yLabel}
      </text>
      {points.map((point, index) => {
        const radius = 6 + Math.sqrt(point.size / maxSize) * 18
        const active = !focus || focus === point.key
        const right = x(point.x) > size * 0.62
        return (
          <g
            key={point.key}
            onMouseEnter={() => onFocus?.(point.key)}
            onMouseLeave={() => onFocus?.(null)}
            className="transition-opacity duration-200"
            style={{ opacity: active ? 1 : 0.3 }}
          >
            <circle
              cx={x(point.x)}
              cy={y(point.y)}
              r={radius}
              className="fill-(--bubble)/35 stroke-(--bubble)"
              style={{ '--bubble': `var(--chart-${(index % 5) + 1})` } as CSSProperties}
            >
              <title>{point.title}</title>
            </circle>
            <text
              x={right ? x(point.x) - radius - 4 : x(point.x) + radius + 4}
              y={y(point.y) + 3}
              textAnchor={right ? 'end' : 'start'}
              className={cn(
                'text-2xs',
                focus === point.key ? 'fill-foreground' : 'fill-muted-foreground',
              )}
            >
              {point.label}
            </text>
          </g>
        )
      })}
    </svg>
  )
}
