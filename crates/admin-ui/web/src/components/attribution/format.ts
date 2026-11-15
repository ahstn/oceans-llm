// Values shared by the attribution components and tabs: number formats, the data-readiness levels
// and harness labels. Kept apart from the components so Fast Refresh can preserve their state.

import { EMPTY_VALUE } from '@/lib/format'

export const PERCENT = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 0,
})
export const PERCENT_1 = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 1,
})
export const SIGNED_PERCENT = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 0,
  signDisplay: 'exceptZero',
})
export const DECIMAL_1 = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1 })

export function formatPercent(value: number | null | undefined, digits: 0 | 1 = 0) {
  if (value == null) return EMPTY_VALUE
  return (digits === 1 ? PERCENT_1 : PERCENT).format(value)
}

/** Lead and review times that scale with size: `25 min` under an hour, `5.2 h`, then `3.1 d`. */
export function formatHours(ms: number | null | undefined) {
  if (ms == null) return EMPTY_VALUE
  const hours = ms / 3_600_000
  if (hours < 1) return `${Math.round(hours * 60)} min`
  return hours < 48 ? `${DECIMAL_1.format(hours)} h` : `${DECIMAL_1.format(hours / 24)} d`
}

/**
 * How far each widget is from real data:
 * - live: the attribution report already returns it.
 * - derived: stored today; needs a new aggregation or endpoint, no new ingestion.
 * - proxy: an approximation from stored data, labelled as such in the UI.
 * - new: needs data Oceans does not collect yet.
 */
export type Readiness = 'live' | 'derived' | 'proxy' | 'new'

export const READINESS: Record<
  Readiness,
  { label: string; variant: 'success' | 'secondary' | 'outline' | 'warning'; detail: string }
> = {
  live: {
    label: 'Live',
    variant: 'success',
    detail: 'The attribution report already returns this.',
  },
  derived: {
    label: 'Derivable',
    variant: 'secondary',
    detail: 'Stored today. Needs a new aggregation in the report, not new ingestion.',
  },
  proxy: {
    label: 'Proxy',
    variant: 'outline',
    detail: 'Approximated from stored data. The page would say what it stands in for.',
  },
  new: {
    label: 'Needs new data',
    variant: 'warning',
    detail: 'Needs data Oceans does not collect yet.',
  },
}

/** Harness names as git-ai notes and Oceans hooks report them, mapped to a label and brand mark. */
export const HARNESSES: Record<string, { label: string; icon?: string }> = {
  claude: { label: 'Claude Code', icon: 'claudecode' },
  claude_code: { label: 'Claude Code', icon: 'claudecode' },
  codex: { label: 'Codex', icon: 'codex' },
  pi: { label: 'Pi', icon: 'pi' },
  oh_my_pi: { label: 'OMP', icon: 'ohmypi' },
  opencode: { label: 'OpenCode', icon: 'opencode' },
  cursor: { label: 'Cursor' },
}

/** git-ai's `claude` and the Claude Code hook's `claude_code` are one harness. */
// Harness names come from git-ai notes as free strings, so lookups never reach inherited
// properties such as `__proto__` or `toString`.
const HARNESS_ALIASES = new Map([['claude_code', 'claude']])

export function canonicalHarness(harness: string) {
  return HARNESS_ALIASES.get(harness) ?? harness
}

/** The label and brand mark for a known harness, else `undefined`. */
export function harnessInfo(harness: string) {
  return Object.hasOwn(HARNESSES, harness) ? HARNESSES[harness] : undefined
}

export function harnessLabel(harness: string) {
  return harnessInfo(harness)?.label ?? harness
}

/** `Nov 14` for a `YYYY-MM-DD` axis key, in UTC so server and browser agree. */
export function formatDay(value: string) {
  return new Date(value).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  })
}
