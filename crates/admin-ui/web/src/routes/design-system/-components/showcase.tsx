import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'

/** Page band with an anchor, so the header nav can jump to it. */
export function ShowcaseSection({
  id,
  title,
  description,
  children,
}: {
  id: string
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <section id={id} className="flex scroll-mt-6 flex-col gap-4">
      <header className="flex flex-col gap-1">
        <h2 className="type-headline">{title}</h2>
        <p className="text-muted-foreground max-w-3xl text-sm">{description}</p>
      </header>
      {children}
    </section>
  )
}

/** One component block: name, when to use it, then the examples. */
export function ShowcaseBlock({
  title,
  description,
  children,
  className,
}: {
  title: string
  description: string
  children: ReactNode
  className?: string
}) {
  return (
    <Card className={cn('min-w-0', className)}>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-4">{children}</CardContent>
    </Card>
  )
}

/** Caption above a row of examples inside a block. */
export function ShowcaseRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <p className="text-muted-foreground type-caption">{label}</p>
      <div className="flex min-w-0 flex-wrap items-center gap-2">{children}</div>
    </div>
  )
}

const defaultStates = ['default', 'hover', 'focus', 'active', 'disabled'] as const

/** Variant × state table. States are pinned through `data-preview`, so no interaction is needed. */
export function StateMatrix<Row extends string>({
  rows,
  states = defaultStates,
  render,
}: {
  rows: readonly Row[]
  states?: readonly string[]
  render: (row: Row, state: string) => ReactNode
}) {
  return (
    <div className="w-full overflow-x-auto">
      <table className="w-full border-separate border-spacing-x-2 border-spacing-y-3">
        <thead>
          <tr>
            <th />
            {states.map((state) => (
              <th
                key={state}
                scope="col"
                className="text-muted-foreground text-left font-mono text-xs font-normal"
              >
                {state}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row}>
              <th
                scope="row"
                className="text-muted-foreground pr-4 text-left font-mono text-xs font-normal"
              >
                {row}
              </th>
              {states.map((state) => (
                <td key={state}>{render(row, state)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// Resolves any CSS color (hex, rgb, oklch, color-mix) to sRGB by painting one pixel.
// Translucent colors are composited over the backdrop, as the browser would.
function toRgb(color: string, backdrop: string) {
  const canvas = document.createElement('canvas')
  canvas.width = 1
  canvas.height = 1
  const context = canvas.getContext('2d', { willReadFrequently: true })
  if (!context) return null
  context.fillStyle = backdrop
  context.fillRect(0, 0, 1, 1)
  context.fillStyle = color
  context.fillRect(0, 0, 1, 1)
  const [r, g, b] = context.getImageData(0, 0, 1, 1).data
  return [r, g, b]
}

function toHex(rgb: number[]) {
  return `#${rgb.map((value) => value.toString(16).padStart(2, '0')).join('')}`
}

function luminance(rgb: number[]) {
  const [r, g, b] = rgb.map((value) => {
    const channel = value / 255
    return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrastRatio(a: number[], b: number[]) {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (light + 0.05) / (dark + 0.05)
}

function wcagLevel(ratio: number) {
  if (ratio >= 7) return 'AAA'
  if (ratio >= 4.5) return 'AA'
  if (ratio >= 3) return 'AA Large'
  return 'Fail'
}

/** A surface with its foreground, the resolved hex, and the measured contrast ratio. */
export function ColorPair({
  name,
  background,
  foreground,
}: {
  name: string
  background: string
  foreground: string
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [info, setInfo] = useState<{ ratio: number; bg: string } | null>(null)

  useEffect(() => {
    const element = ref.current
    if (!element) return
    const style = getComputedStyle(element)
    const page = getComputedStyle(document.body).backgroundColor
    const bg = toRgb(style.backgroundColor, page)
    if (!bg) return
    const fg = toRgb(style.color, toHex(bg))
    if (fg) setInfo({ ratio: contrastRatio(bg, fg), bg: toHex(bg) })
  }, [])

  const tokens = {
    '--pair-bg': `var(--${background})`,
    '--pair-fg': `var(--${foreground})`,
  } as CSSProperties

  return (
    <div className="flex min-w-0 flex-col overflow-hidden rounded-xl border">
      <div
        ref={ref}
        style={tokens}
        className="flex h-24 items-end justify-between bg-(--pair-bg) p-4 text-(--pair-fg)"
      >
        <span className="text-2xl font-medium">Aa</span>
        {info ? (
          <span
            data-testid="contrast-ratio"
            className="rounded-full border border-current/20 px-2 py-0.5 font-mono text-xs"
          >
            {info.ratio.toFixed(2)} {wcagLevel(info.ratio)}
          </span>
        ) : null}
      </div>
      <div className="bg-background flex flex-col gap-0.5 p-3">
        <span className="text-sm font-medium">{name}</span>
        <span className="text-muted-foreground truncate font-mono text-xs">
          --{background} / --{foreground}
        </span>
        {info ? <span className="text-muted-foreground font-mono text-xs">{info.bg}</span> : null}
      </div>
    </div>
  )
}

/** A single token swatch, labelled with its DESIGN.md name and CSS variable. */
export function Swatch({ name, token, note }: { name: string; token: string; note?: string }) {
  const style = { '--swatch': `var(--${token})` } as CSSProperties
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div style={style} className="h-14 rounded-lg border bg-(--swatch)" />
      <div className="flex min-w-0 flex-col">
        <span className="truncate text-sm font-medium">{name}</span>
        <span className="text-muted-foreground truncate font-mono text-xs">--{token}</span>
        {note ? <span className="text-muted-foreground truncate text-xs">{note}</span> : null}
      </div>
    </div>
  )
}

/** Side-by-side rule illustration. "Don't" is marked with destructive text, not a new color. */
export function DoDont({ rule, good, bad }: { rule: string; good: ReactNode; bad: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-3">
      <p className="text-sm font-medium">{rule}</p>
      <div className="grid gap-3 md:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-2 rounded-xl border p-4">
          <span className="type-caption text-success">Do</span>
          <div className="flex min-w-0 flex-wrap items-center gap-2">{good}</div>
        </div>
        <div className="flex min-w-0 flex-col gap-2 rounded-xl border p-4">
          <span className="type-caption text-destructive">Don&apos;t</span>
          <div className="flex min-w-0 flex-wrap items-center gap-2">{bad}</div>
        </div>
      </div>
    </div>
  )
}
