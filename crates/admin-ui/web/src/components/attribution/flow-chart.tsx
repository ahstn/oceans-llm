// A column Sankey drawn in SVG: nodes stacked per stage, ribbons sized by value. Recharts' Sankey
// lays nodes out itself; outcome flows need fixed stage columns and labelled drop-offs, so this
// lays them out directly. Hovering a node or ribbon dims everything not connected to it.

import { useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react'

import { cn } from '@/lib/utils'

export type FlowNode = {
  id: string
  label: string
  column: number
  value: number
  tone: string
  /** Second label line, such as `36 · 21% · $2,856`. */
  detail?: string
  /** Drop-off nodes sit below the main stack, muted. */
  leak?: boolean
}

export type FlowLink = { source: string; target: string; value: number }

type LaidNode = FlowNode & { x: number; y: number; height: number }

const NODE_WIDTH = 10
const NODE_GAP = 18
const LABEL_WIDTH = 150

// Layout, focus walk and drawing share the same scale and positions, so they stay together.
// oxlint-disable-next-line eslint/max-lines-per-function
export function FlowChart({
  nodes,
  links,
  columns,
  height = 400,
  ariaLabel,
}: {
  nodes: FlowNode[]
  links: FlowLink[]
  /** Stage headings, one per column. */
  columns: { title: string; value?: string }[]
  height?: number
  ariaLabel: string
}) {
  // Drawn at its rendered width, so labels stay at their real type size instead of scaling.
  const frameRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(960)
  useLayoutEffect(() => {
    const frame = frameRef.current
    if (!frame || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(([entry]) =>
      setWidth(Math.max(560, entry.contentRect.width)),
    )
    observer.observe(frame)
    return () => observer.disconnect()
  }, [])
  const top = 48
  const [focus, setFocus] = useState<string | null>(null)

  const layout = useMemo(() => {
    const columnCount = columns.length
    const usable = width - LABEL_WIDTH - NODE_WIDTH
    const columnX = (column: number) => (usable / Math.max(1, columnCount - 1)) * column
    const byColumn = new Map<number, FlowNode[]>()
    for (const node of nodes)
      byColumn.set(node.column, [...(byColumn.get(node.column) ?? []), node])
    // One scale for every column: the tightest column (value plus its gaps) fills the height.
    const available = height - top - 8
    const scale = Math.min(
      ...[...byColumn.values()].map(
        (column) =>
          (available - (column.length - 1) * NODE_GAP) /
          Math.max(
            1,
            column.reduce((total, node) => total + node.value, 0),
          ),
      ),
    )
    const laid = new Map<string, LaidNode>()
    for (const [column, list] of byColumn) {
      let y = top
      const ordered = [...list].sort((a, b) => Number(Boolean(a.leak)) - Number(Boolean(b.leak)))
      for (const node of ordered) {
        const nodeHeight = Math.max(2, node.value * scale)
        laid.set(node.id, { ...node, x: columnX(column), y, height: nodeHeight })
        y += nodeHeight + NODE_GAP
      }
    }
    // Ribbons leave and enter each node in the order of their partner's position.
    const outOffset = new Map<string, number>()
    const inOffset = new Map<string, number>()
    const ribbons = [...links]
      .sort((a, b) => (laid.get(a.target)?.y ?? 0) - (laid.get(b.target)?.y ?? 0))
      .map((link) => {
        const source = laid.get(link.source)
        const target = laid.get(link.target)
        if (!source || !target) return null
        const thickness = Math.max(1, link.value * scale)
        const sy = source.y + (outOffset.get(source.id) ?? 0)
        outOffset.set(source.id, (outOffset.get(source.id) ?? 0) + thickness)
        return { link, source, target, thickness, sy }
      })
      .filter((ribbon): ribbon is NonNullable<typeof ribbon> => Boolean(ribbon))
      .sort((a, b) => a.sy - b.sy)
      .map((ribbon) => {
        const ty = ribbon.target.y + (inOffset.get(ribbon.target.id) ?? 0)
        inOffset.set(ribbon.target.id, (inOffset.get(ribbon.target.id) ?? 0) + ribbon.thickness)
        const x0 = ribbon.source.x + NODE_WIDTH
        const x1 = ribbon.target.x
        const mid = (x0 + x1) / 2
        const t = ribbon.thickness
        const path = `M${x0},${ribbon.sy}C${mid},${ribbon.sy} ${mid},${ty} ${x1},${ty}L${x1},${ty + t}C${mid},${ty + t} ${mid},${ribbon.sy + t} ${x0},${ribbon.sy + t}Z`
        return { ...ribbon, path }
      })
    return { laid: [...laid.values()], ribbons, columnX }
  }, [nodes, links, columns.length, height, width])

  const connected = useMemo(() => {
    if (!focus) return null
    const set = new Set<string>([focus])
    // Walk both directions so a middle node lights its whole lineage.
    let frontier = [focus]
    while (frontier.length) {
      const next: string[] = []
      for (const id of frontier)
        for (const link of links)
          if (link.source === id && !set.has(link.target)) {
            set.add(link.target)
            next.push(link.target)
          }
      frontier = next
    }
    frontier = [focus]
    while (frontier.length) {
      const next: string[] = []
      for (const id of frontier)
        for (const link of links)
          if (link.target === id && !set.has(link.source)) {
            set.add(link.source)
            next.push(link.source)
          }
      frontier = next
    }
    return set
  }, [focus, links])

  return (
    <div ref={frameRef} className="w-full min-w-0 overflow-x-auto">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        height={height}
        className="block"
        role="img"
        aria-label={ariaLabel}
      >
        {columns.map((column, index) => (
          <g key={column.title} transform={`translate(${layout.columnX(index)},0)`}>
            <text y={14} className="fill-muted-foreground text-xs">
              {column.title}
            </text>
            {column.value ? (
              <text y={32} className="fill-foreground text-sm font-medium">
                {column.value}
              </text>
            ) : null}
          </g>
        ))}
        <g>
          {layout.ribbons.map((ribbon) => {
            const active =
              !connected || (connected.has(ribbon.source.id) && connected.has(ribbon.target.id))
            return (
              <path
                key={`${ribbon.link.source}-${ribbon.link.target}`}
                d={ribbon.path}
                className="fill-(--ribbon) transition-opacity duration-200"
                style={
                  {
                    '--ribbon': ribbon.target.tone,
                    opacity: active ? (ribbon.target.leak ? 0.22 : 0.32) : 0.06,
                  } as CSSProperties
                }
                onMouseEnter={() => setFocus(ribbon.target.id)}
                onMouseLeave={() => setFocus(null)}
              >
                <title>{`${ribbon.source.label} → ${ribbon.target.label}`}</title>
              </path>
            )
          })}
        </g>
        {layout.laid.map((node) => {
          const active = !connected || connected.has(node.id)
          return (
            <g
              key={node.id}
              className="transition-opacity duration-200"
              style={{ opacity: active ? 1 : 0.35 }}
              onMouseEnter={() => setFocus(node.id)}
              onMouseLeave={() => setFocus(null)}
            >
              <rect
                x={node.x}
                y={node.y}
                width={NODE_WIDTH}
                height={node.height}
                rx={2}
                className="fill-(--node)"
                style={{ '--node': node.tone } as CSSProperties}
              />
              <text
                x={node.x + NODE_WIDTH + 8}
                y={node.y + Math.min(node.height / 2, 14) + 4}
                className={cn(
                  'text-xs font-medium',
                  node.leak ? 'fill-muted-foreground' : 'fill-foreground',
                )}
              >
                {node.label}
              </text>
              {node.detail ? (
                <text
                  x={node.x + NODE_WIDTH + 8}
                  y={node.y + Math.min(node.height / 2, 14) + 19}
                  className="fill-muted-foreground text-2xs tabular-nums"
                >
                  {node.detail}
                </text>
              ) : null}
            </g>
          )
        })}
      </svg>
    </div>
  )
}
