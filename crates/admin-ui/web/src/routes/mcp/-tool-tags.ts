import type { McpToolAnnotationsView } from '@/types/api'

export type HintKey = keyof McpToolAnnotationsView

export const toolHints: { key: HintKey; label: string }[] = [
  { key: 'read_only_hint', label: 'Read-only' },
  { key: 'destructive_hint', label: 'Destructive' },
  { key: 'idempotent_hint', label: 'Idempotent' },
  { key: 'open_world_hint', label: 'Open world' },
]

/** Tags derived from the effective hints; admin tags cannot use these names. */
export const derivedTags = ['read_only', 'destructive', 'idempotent', 'open_world'] as const

const derivedTagSet: ReadonlySet<string> = new Set(derivedTags)

export function isDerivedTag(tag: string) {
  return derivedTagSet.has(tag)
}

/** Select value meaning "no override, use the upstream hint". */
export const UPSTREAM = 'upstream'

export function hintValue(value: boolean | null | undefined) {
  if (value === true) {
    return 'true'
  }
  if (value === false) {
    return 'false'
  }
  return UPSTREAM
}

export function parseHint(value: string): boolean | null {
  if (value === 'true') {
    return true
  }
  if (value === 'false') {
    return false
  }
  return null
}

export function describeHint(value: boolean | null | undefined) {
  if (value === true) {
    return 'Yes'
  }
  if (value === false) {
    return 'No'
  }
  return 'Not declared'
}

/** Admin tags as typed: split on commas and whitespace, without empties. */
export function parseTagInput(value: string) {
  return value.split(/[\s,]+/).filter(Boolean)
}
