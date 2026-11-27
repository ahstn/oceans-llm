import type { IconSvgElement } from '@hugeicons/react'

import { formatUtcDate } from '@/lib/format'

import type { FilterOption } from './filter-options'

// Field definitions for `FilterToolbar`, kept apart from the component so the component module
// exports only components.

/** Chip drafts and URL patches: each param's typed value, or `undefined` to remove it. */
export type FilterPatch<Param extends string> = {
  [Key in Param]?: string | number | null | undefined
}

/**
 * Where a picker's options come from: a fixed list, `'dates'` for recent UTC days, or the name of
 * a catalog that the page's `useCatalogOptions` hook loads once the chip shows.
 */
export type PickerSource = FilterOption[] | string

export interface FilterPicker {
  source: PickerSource
  searchPlaceholder: string
  /** Turns typed text into a value the list lacks, or `null` when only listed values apply. */
  customValue: (search: string) => string | null
  /** Renders a value that is not in the list. */
  formatValue?: (value: string) => string
  /** Turns a URL value into a picker value, such as a timestamp into its UTC day. */
  fromParam?: (value: string) => string
  /** Turns a picked value into its URL value. */
  toParam?: (value: string) => string
}

/** A key and value typed side by side, such as a request tag. Both must be set to apply. */
export interface FilterPair {
  keyLabel: string
  keyPlaceholder: string
  valueLabel: string
  incompleteTitle: string
  incompleteDescription: string
}

export interface FilterField<Param extends string> {
  key: string
  label: string
  icon: IconSvgElement
  placeholder: string
  /** The URL params the chip owns: one, or the key and value of a pair. */
  params: [Param] | [Param, Param]
  /** Fields with a known set of values pick from a list instead of free text. */
  picker?: FilterPicker
  pair?: FilterPair
}

export interface FilterGroup<Param extends string> {
  label: string
  fields: FilterField<Param>[]
}

export function groupParams<Param extends string>(groups: FilterGroup<Param>[]): Param[] {
  return groups.flatMap((group) => group.fields.flatMap((field) => field.params))
}

/** A URL patch that removes every filter the groups own and leaves other params alone. */
export function clearedFilters<Param extends string>(
  groups: FilterGroup<Param>[],
): Partial<Record<Param, undefined>> {
  return Object.fromEntries(groupParams(groups).map((param) => [param, undefined])) as Partial<
    Record<Param, undefined>
  >
}

export function hasFilters<Param extends string>(
  groups: FilterGroup<Param>[],
  values: FilterPatch<Param>,
) {
  return groupParams(groups).some((param) => {
    const value = values[param]
    return value !== undefined && value !== null && String(value).trim() !== ''
  })
}

/** Picker for a closed set of values: nothing outside the list applies. */
export const strictValue = () => null

function humanize(value: string) {
  return value.replaceAll('_', ' ').replace(/^\w/, (character) => character.toUpperCase())
}

/** Options for an API enum, labelled in sentence case (`policy_denied` reads `Policy denied`). */
export function enumOptions(values: readonly string[]): FilterOption[] {
  return values.map((value) => ({ value, label: humanize(value) }))
}

/** A typed `YYYY-MM-DD` day that exists on the calendar. */
export function customDay(search: string) {
  const day = search.trim()
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return null
  const date = new Date(`${day}T00:00:00.000Z`)
  return !Number.isNaN(date.getTime()) && date.toISOString().startsWith(day) ? day : null
}

const dayPresets: Array<{ daysAgo: number; hint: string }> = [
  { daysAgo: 0, hint: 'Today' },
  { daysAgo: 1, hint: 'Yesterday' },
  { daysAgo: 7, hint: '7 days ago' },
  { daysAgo: 30, hint: '30 days ago' },
  { daysAgo: 90, hint: '90 days ago' },
]

/** Recent UTC days to start from; any other day can be typed as `YYYY-MM-DD`. */
export function dayOptions(now: Date): FilterOption[] {
  return dayPresets.map(({ daysAgo, hint }) => {
    const day = new Date(now.getTime() - daysAgo * 24 * 60 * 60 * 1000).toISOString().slice(0, 10)
    return { value: day, label: formatUtcDate(`${day}T00:00:00.000Z`), hint }
  })
}

const DAY_MS = 24 * 60 * 60 * 1000

/** The UTC day `days` away from `day`, both as `YYYY-MM-DD`. */
function shiftDay(day: string, days: number) {
  return new Date(Date.parse(`${day}T00:00:00.000Z`) + days * DAY_MS).toISOString().slice(0, 10)
}

/**
 * A UTC day picker. Every boundary keeps the whole day it names, so `after 3 Nov` and
 * `before 3 Nov` both include 3 Nov. `end` sends the day's last millisecond for stores that
 * compare with `<=`; `end-exclusive` sends the next midnight for stores that compare with `<`
 * on whole seconds, such as batches, where `23:59:59.999` would drop the day's last second.
 */
export function dayPicker(boundary: 'start' | 'end' | 'end-exclusive'): FilterPicker {
  return {
    source: 'dates',
    searchPlaceholder: 'Search or type YYYY-MM-DD…',
    customValue: customDay,
    formatValue: (day) => formatUtcDate(`${day}T00:00:00.000Z`),
    fromParam: (timestamp) =>
      boundary === 'end-exclusive' ? shiftDay(timestamp.slice(0, 10), -1) : timestamp.slice(0, 10),
    toParam: (day) => {
      if (boundary === 'start') return `${day}T00:00:00.000Z`
      if (boundary === 'end') return `${day}T23:59:59.999Z`
      return `${shiftDay(day, 1)}T00:00:00.000Z`
    },
  }
}
