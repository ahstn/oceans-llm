// AI share bands, shared by the live Attribution tabs and their concept mockups so the same share
// always lands in the same band.

/** AI share bands: more honest than an AI-assisted yes or no. */
// `No AI` is exactly zero AI lines; any AI line puts a pull request in a band above it.
export const AI_BANDS = [
  { key: 'none', label: 'No AI', min: 0, max: 0 },
  { key: 'light', label: 'Under 50% AI', min: 0, max: 0.5 },
  { key: 'heavy', label: '50–89% AI', min: 0.5, max: 0.9 },
  { key: 'agentic', label: '90%+ AI', min: 0.9, max: 1.01 },
] as const
export type AiBandKey = (typeof AI_BANDS)[number]['key']

export function aiBand(share: number): AiBandKey {
  if (share <= 0) return 'none'
  const band = AI_BANDS.find(
    (entry) => entry.key !== 'none' && share >= entry.min && share < entry.max,
  )
  return band?.key ?? 'agentic'
}
