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
 * Line diff by longest common subsequence. Tool definitions are small, so the
 * quadratic table is fine and keeps the output easy to read.
 */
export function lineDiff(before: string, after: string): DiffLine[] {
  const left = before ? before.split('\n') : []
  const right = after ? after.split('\n') : []
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
