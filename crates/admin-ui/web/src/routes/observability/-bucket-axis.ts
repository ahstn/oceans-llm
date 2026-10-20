/**
 * X-axis helpers shared by the Leaderboard and Agent harnesses charts, which plot 12-hour buckets
 * aligned to UTC midnight and noon.
 */

const AXIS_DAY_FORMATTER = new Intl.DateTimeFormat('en-US', {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
})

/** Buckets that start a UTC day, so each day gets one tick instead of one per bucket. */
export function utcDayTicks(bucketStarts: string[]) {
  return bucketStarts.filter((value) => new Date(value).getUTCHours() === 0)
}

/** Day tick label, e.g. `Oct 16`, matching the other daily charts. */
export function formatBucketDay(value: string) {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '' : AXIS_DAY_FORMATTER.format(date)
}
