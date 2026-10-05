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
