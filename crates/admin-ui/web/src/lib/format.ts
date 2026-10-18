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
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
})

/** Formats a Money4 amount at full ten-thousandth precision, e.g. `$0.0457` for a single request. */
export function formatUsd10000Precise(amountUsd10000: number) {
  return PRECISE_CURRENCY_FORMATTER.format(amountUsd10000 / 10_000)
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

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

/**
 * Formats an instant as `16 Oct 2026, 02:04 UTC`, the DESIGN.md timestamp form. Fixed UTC parts
 * keep server and browser output identical during hydration, whatever their ICU data.
 */
export function formatUtcTimestamp(value: Date | string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return '—'
  }
  const hours = String(date.getUTCHours()).padStart(2, '0')
  const minutes = String(date.getUTCMinutes()).padStart(2, '0')
  return `${date.getUTCDate()} ${MONTHS[date.getUTCMonth()]} ${date.getUTCFullYear()}, ${hours}:${minutes} UTC`
}
