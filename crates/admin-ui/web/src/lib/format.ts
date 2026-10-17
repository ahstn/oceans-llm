// Shared formats for numbers, money, durations and timestamps. DESIGN.md "Numbers and time" sets
// the rule: each kind of value has one format, so a column compares at a glance.

/** Shown wherever a value is missing. Never `n/a`. */
export const EMPTY_VALUE = '—'

export const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

/** Formats an API amount expressed in ten-thousandths of a dollar. */
export function formatUsd10000(amountUsd10000: number) {
  return CURRENCY_FORMATTER.format(amountUsd10000 / 10_000)
}

const PRECISE_CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 4,
  maximumFractionDigits: 4,
})

/**
 * Formats a Money4 amount at full ten-thousandth precision, e.g. `$0.0457` for a single request.
 * Always four decimals, so a column of request costs lines up.
 */
export function formatUsd10000Precise(amountUsd10000: number) {
  return PRECISE_CURRENCY_FORMATTER.format(amountUsd10000 / 10_000)
}

const COUNT_FORMATTER = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const COMPACT_COUNT_FORMATTER = new Intl.NumberFormat('en-US', {
  notation: 'compact',
  maximumFractionDigits: 1,
})

/** Counts from this size up go compact (`34.4M`), with the exact count shown on hover. */
export const COMPACT_COUNT_THRESHOLD = 100_000

/** A count with grouped digits: `9,812`. */
export function formatCount(value: number | null | undefined) {
  return value == null ? EMPTY_VALUE : COUNT_FORMATTER.format(value)
}

/** A count for a table cell: grouped below 100,000 (`9,812`), compact from there (`34.4M`). */
export function formatCompactCount(value: number | null | undefined) {
  if (value == null) {
    return EMPTY_VALUE
  }
  return Math.abs(value) >= COMPACT_COUNT_THRESHOLD
    ? COMPACT_COUNT_FORMATTER.format(value)
    : COUNT_FORMATTER.format(value)
}

/** A duration that carries its unit and scales with size: `810 ms`, `3.96 s`, `1 h 6 min`. */
export function formatDuration(milliseconds: number | null | undefined) {
  if (milliseconds == null || !Number.isFinite(milliseconds)) {
    return EMPTY_VALUE
  }
  const ms = Math.max(0, milliseconds)
  if (ms < 1_000) {
    return `${Math.round(ms)} ms`
  }
  if (ms < 60_000) {
    // Three significant figures: 3.96 s, 12.4 s, 59.9 s.
    const seconds = ms / 1_000
    const digits = seconds < 10 ? 2 : 1
    return `${(Math.floor(seconds * 10 ** digits) / 10 ** digits).toFixed(digits)} s`
  }
  const totalSeconds = Math.floor(ms / 1_000)
  if (totalSeconds < 3_600) {
    const minutes = Math.floor(totalSeconds / 60)
    const seconds = totalSeconds % 60
    return seconds > 0 ? `${minutes} min ${seconds} s` : `${minutes} min`
  }
  const hours = Math.floor(totalSeconds / 3_600)
  const minutes = Math.floor((totalSeconds % 3_600) / 60)
  return minutes > 0 ? `${hours} h ${minutes} min` : `${hours} h`
}

// Fixed English month names and UTC parts, rather than Intl, keep server and browser output
// identical during hydration and avoid locale data differences such as `Sept`.
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

function pad(value: number) {
  return String(value).padStart(2, '0')
}

/**
 * A timestamp in UTC: `16 Oct 2026, 02:04 UTC`. Pass `seconds` only where order within a minute
 * matters, as in request logs (`16 Oct 2026, 02:04:09 UTC`). Render it `whitespace-nowrap`.
 */
export function formatTimestamp(
  value: Date | string | null | undefined,
  options: { seconds?: boolean } = {},
) {
  if (value == null || value === '') {
    return EMPTY_VALUE
  }
  const date = value instanceof Date ? value : new Date(value)
  if (Number.isNaN(date.getTime())) {
    return EMPTY_VALUE
  }
  const time = `${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}${
    options.seconds ? `:${pad(date.getUTCSeconds())}` : ''
  }`
  return `${date.getUTCDate()} ${MONTHS[date.getUTCMonth()]} ${date.getUTCFullYear()}, ${time} UTC`
}

const EXPIRY_FORMATTER = new Intl.DateTimeFormat('en-US', {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
})

/** The last day a temporary budget increase is in effect; expiry is the exclusive boundary. */
export function formatIncreaseLastDay(expiresAt: Date | string) {
  return EXPIRY_FORMATTER.format(new Date(new Date(expiresAt).getTime() - 1))
}
