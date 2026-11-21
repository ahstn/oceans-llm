// SVG paths for a sparkline with gaps: one line segment and one closed area per run of values.

type Point = readonly [number, number]

/**
 * The line through every run of consecutive points, and an area closed down to `baseline` under
 * each run on its own, so a gap (a `null` point) is never painted across.
 */
export function sparklinePaths(points: (Point | null)[], baseline: number) {
  const runs: Point[][] = []
  let run: Point[] = []
  for (const point of points) {
    if (point) {
      run.push(point)
      continue
    }
    if (run.length) runs.push(run)
    run = []
  }
  if (run.length) runs.push(run)
  const segment = (points: Point[]) =>
    points
      .map(([x, y], index) => `${index === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`)
      .join('')
  let line = ''
  let area = ''
  for (const points of runs) {
    const path = segment(points)
    line += path
    // A lone point has no width to fill under.
    if (points.length < 2) continue
    const first = points[0]
    const last = points[points.length - 1]
    area += `${path}L${last[0].toFixed(2)},${baseline}L${first[0].toFixed(2)},${baseline}Z`
  }
  return { line, area }
}
