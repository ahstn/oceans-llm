// Delivery performance bands, shared by the live Delivery tab and its concept mockup. Pure rules on
// plain numbers, so neither route depends on the other's data adapters.

export type DoraBand = 'Elite' | 'High' | 'Medium' | 'Low'

/** DORA 2024 bands for deployment frequency, from deploys (here merges) per day. */
export function deployBand(perDay: number): DoraBand {
  if (perDay >= 1) return 'Elite'
  if (perDay >= 1 / 7) return 'High'
  if (perDay >= 1 / 30) return 'Medium'
  return 'Low'
}

/** DORA 2024 bands for lead time, in hours. */
export function leadBand(hours: number | null): DoraBand | undefined {
  if (hours == null) return undefined
  if (hours < 24) return 'Elite'
  if (hours < 24 * 7) return 'High'
  if (hours < 24 * 30) return 'Medium'
  return 'Low'
}

/**
 * Change-fail-rate bands of Oceans' own, ordered by rate. DORA 2024's change-fail clusters are not
 * ordered by performance (its High cluster fails more often than Medium), so these are not its values.
 */
export function failBand(rate: number | null): DoraBand | undefined {
  if (rate == null) return undefined
  if (rate <= 0.05) return 'Elite'
  if (rate <= 0.1) return 'High'
  if (rate <= 0.15) return 'Medium'
  return 'Low'
}
