import { render } from '@testing-library/react'
import { Area, AreaChart, Bar, BarChart } from 'recharts'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { chartAnimation } from '@/lib/chart-motion'

const rows = [
  { day: 'Mon', requests: 4 },
  { day: 'Tue', requests: 7 },
  { day: 'Wed', requests: 5 },
]

// Recharts reads the preference through matchMedia when a chart mounts.
function preferReducedMotion(reduce: boolean) {
  vi.spyOn(window, 'matchMedia').mockImplementation((query: string) => ({
    matches: reduce && query === '(prefers-reduced-motion: reduce)',
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }))
}

// Bars draw in by growing from nothing, so a first render that animates has no bars yet.
function barsOnFirstRender(reduce: boolean) {
  preferReducedMotion(reduce)
  const { container } = render(
    <BarChart width={400} height={200} data={rows}>
      <Bar dataKey="requests" {...chartAnimation} />
    </BarChart>,
  )
  return container.querySelectorAll('path.recharts-rectangle').length
}

// Areas draw in behind a clip rectangle that widens from zero.
function areaHiddenOnFirstRender(reduce: boolean) {
  preferReducedMotion(reduce)
  const { container } = render(
    <AreaChart width={400} height={200} data={rows}>
      <Area dataKey="requests" {...chartAnimation} />
    </AreaChart>,
  )
  // getElementsByTagName: jsdom's selector engine misses camel-case SVG tags inside combinators.
  return [...container.getElementsByTagName('clipPath')].some(
    (clip) => clip.querySelector('rect')?.getAttribute('width') === '0',
  )
}

describe('chartAnimation under prefers-reduced-motion', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('draws every bar on the first render, with no draw-in', () => {
    expect(barsOnFirstRender(true)).toBe(rows.length)
    // Without the preference the same chart starts its draw-in from no bars.
    expect(barsOnFirstRender(false)).toBe(0)
  })

  it('shows the whole area on the first render, with no reveal', () => {
    expect(areaHiddenOnFirstRender(true)).toBe(false)
    // Without the preference the area starts hidden behind a zero-width clip.
    expect(areaHiddenOnFirstRender(false)).toBe(true)
  })
})
