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
