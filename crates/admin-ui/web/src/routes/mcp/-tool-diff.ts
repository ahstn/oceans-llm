export type DiffLine = { kind: 'same' | 'added' | 'removed'; text: string }

/** Pretty-prints a JSON value for a line diff; absent values render as nothing. */
export function diffText(value: unknown): string {
  if (value === null || value === undefined) {
    return ''
  }
  if (typeof value === 'string') {
    return value
  }
  return JSON.stringify(sortKeys(value), null, 2)
}

function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortKeys)
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, entry]) => [key, sortKeys(entry)]),
    )
  }
  return value
}

/**
 * Most cells the longest-common-subsequence table may have. Above this, the
 * changed middle is shown as removed then added, so a huge schema cannot
 * freeze the page.
 */
export const MAX_DIFF_CELLS = 250_000

/**
 * Line diff. Lines shared at the start and end are matched directly; the
 * changed middle uses a longest-common-subsequence table when it is small
 * enough, and is shown as removed then added otherwise.
 */
export function lineDiff(before: string, after: string): DiffLine[] {
  const left = before ? before.split('\n') : []
  const right = after ? after.split('\n') : []
  let prefix = 0
  while (prefix < left.length && prefix < right.length && left[prefix] === right[prefix]) {
    prefix += 1
  }
  let suffix = 0
  while (
    suffix < left.length - prefix &&
    suffix < right.length - prefix &&
    left[left.length - 1 - suffix] === right[right.length - 1 - suffix]
  ) {
    suffix += 1
  }
  const leftMiddle = left.slice(prefix, left.length - suffix)
  const rightMiddle = right.slice(prefix, right.length - suffix)
  const middle =
    (leftMiddle.length + 1) * (rightMiddle.length + 1) > MAX_DIFF_CELLS
      ? [
          ...leftMiddle.map((text): DiffLine => ({ kind: 'removed', text })),
          ...rightMiddle.map((text): DiffLine => ({ kind: 'added', text })),
        ]
      : lcsDiff(leftMiddle, rightMiddle)
  return [
    ...left.slice(0, prefix).map((text): DiffLine => ({ kind: 'same', text })),
    ...middle,
    ...left.slice(left.length - suffix).map((text): DiffLine => ({ kind: 'same', text })),
  ]
}

function lcsDiff(left: string[], right: string[]): DiffLine[] {
  const lengths = Array.from({ length: left.length + 1 }, () =>
    Array.from({ length: right.length + 1 }, () => 0),
  )
  for (let i = left.length - 1; i >= 0; i -= 1) {
    for (let j = right.length - 1; j >= 0; j -= 1) {
      lengths[i][j] =
        left[i] === right[j]
          ? lengths[i + 1][j + 1] + 1
          : Math.max(lengths[i + 1][j], lengths[i][j + 1])
    }
  }
  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < left.length && j < right.length) {
    if (left[i] === right[j]) {
      lines.push({ kind: 'same', text: left[i] })
      i += 1
      j += 1
    } else if (lengths[i + 1][j] >= lengths[i][j + 1]) {
      lines.push({ kind: 'removed', text: left[i] })
      i += 1
    } else {
      lines.push({ kind: 'added', text: right[j] })
      j += 1
    }
  }
  for (; i < left.length; i += 1) {
    lines.push({ kind: 'removed', text: left[i] })
  }
  for (; j < right.length; j += 1) {
    lines.push({ kind: 'added', text: right[j] })
  }
  return lines
}
