import type { IconSvgElement } from '@hugeicons/react'
import {
  AiChipIcon,
  ApiIcon,
  Calendar04Icon,
  CalendarAdd01Icon,
  CheckListIcon,
  DashboardSpeed01Icon,
  FingerPrintIcon,
  Flag02Icon,
  HashIcon,
  Robot01Icon,
  StatusIcon,
  Tag01Icon,
  Target02Icon,
  UserAccountIcon,
  UserGroupIcon,
  UserIcon,
  UserSquareIcon,
} from '@hugeicons/core-free-icons'

import { anyTypedValue, type FilterOption } from '@/components/data/filter-options'
import { formatUtcDate } from '@/lib/format'
import type { AgentSessionFiltersInput } from '@/types/api'

/** Every agent session filter the URL search carries; paging stays with the table. */
export type SessionFilterValues = Omit<AgentSessionFiltersInput, 'page' | 'page_size'>
export type SessionFilterParam = keyof SessionFilterValues
/** Chips own one param each, except the request tag chip, which owns its key and value. */
export type SessionFilterFieldKey =
  | Exclude<SessionFilterParam, 'request_tag_key' | 'request_tag_value'>
  | 'request_tag'

/** Where a picker's options come from: a fixed list, or a catalog loaded once the chip shows. */
export type PickerSource = FilterOption[] | 'harnesses' | 'models' | 'dates'

export interface SessionFilterPicker {
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

export interface SessionFilterField {
  key: SessionFilterFieldKey
  label: string
  icon: IconSvgElement
  placeholder: string
  /** Fields with a known set of values pick from a list instead of free text. */
  picker?: SessionFilterPicker
}

export interface SessionFilterGroup {
  label: string
  fields: SessionFilterField[]
}

function humanize(value: string) {
  return value.replaceAll('_', ' ').replace(/^\w/, (character) => character.toUpperCase())
}

function enumOptions(values: string[]): FilterOption[] {
  return values.map((value) => ({ value, label: humanize(value) }))
}

/** Values the gateway writes today; older or newer data can hold others, so typing is allowed. */
const operationOptions: FilterOption[] = [
  'chat_completions',
  'messages',
  'responses',
  'embeddings',
  'images',
  'decisions',
].map((value) => ({ value, label: value }))

const callerClassOptions: FilterOption[] = [
  { value: 'user', label: 'User' },
  { value: 'service_account', label: 'Service account' },
  { value: 'api_key', label: 'API key' },
]

const coverageOptions: FilterOption[] = [25, 50, 75, 90, 100].map((percent) => ({
  value: String(percent),
  label: `${percent}%`,
}))

/** A whole percentage from 0 to 100, typed with or without `%`. */
export function customCoveragePercent(search: string) {
  const digits = search.trim().replace(/%$/, '').trim()
  if (!/^\d{1,3}$/.test(digits)) return null
  const percent = Number(digits)
  return percent <= 100 ? String(percent) : null
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

function dayPicker(boundary: 'start' | 'end'): SessionFilterPicker {
  return {
    source: 'dates',
    searchPlaceholder: 'Search or type YYYY-MM-DD…',
    customValue: customDay,
    formatValue: (day) => formatUtcDate(`${day}T00:00:00.000Z`),
    fromParam: (timestamp) => timestamp.slice(0, 10),
    // `Started after` includes the whole day it names, and so does `Started before`.
    toParam: (day) => `${day}${boundary === 'start' ? 'T00:00:00.000Z' : 'T23:59:59.999Z'}`,
  }
}

const strictValue = () => null

export const sessionFilterGroups: SessionFilterGroup[] = [
  {
    label: 'Session',
    fields: [
      {
        key: 'lifecycle',
        label: 'State',
        icon: StatusIcon,
        placeholder: 'Select a state',
        picker: {
          source: enumOptions(['open', 'finalized']),
          searchPlaceholder: 'Search states…',
          customValue: strictValue,
        },
      },
      {
        key: 'gateway_outcome',
        label: 'Outcome',
        icon: Flag02Icon,
        placeholder: 'Select an outcome',
        picker: {
          source: enumOptions(['succeeded', 'partial', 'failed', 'unknown']),
          searchPlaceholder: 'Search outcomes…',
          customValue: strictValue,
        },
      },
      {
        key: 'harness_key',
        label: 'Harness',
        icon: Robot01Icon,
        placeholder: 'Select a harness',
        picker: {
          source: 'harnesses',
          searchPlaceholder: 'Search or type a harness key…',
          customValue: anyTypedValue,
        },
      },
      {
        key: 'requested_model_key',
        label: 'Model',
        icon: AiChipIcon,
        placeholder: 'Select a model',
        picker: {
          source: 'models',
          searchPlaceholder: 'Search models…',
          customValue: anyTypedValue,
        },
      },
      {
        key: 'operation',
        label: 'Operation',
        icon: ApiIcon,
        placeholder: 'Select an operation',
        picker: {
          source: operationOptions,
          searchPlaceholder: 'Search or type an operation…',
          customValue: anyTypedValue,
        },
      },
      {
        key: 'caller_class',
        label: 'Caller class',
        icon: UserSquareIcon,
        placeholder: 'Select a caller class',
        picker: {
          source: callerClassOptions,
          searchPlaceholder: 'Search caller classes…',
          customValue: anyTypedValue,
        },
      },
    ],
  },
  {
    label: 'Score',
    fields: [
      {
        key: 'score_maturity',
        label: 'Score maturity',
        icon: Target02Icon,
        placeholder: 'Select a maturity',
        picker: {
          source: enumOptions(['experimental', 'calibrated']),
          searchPlaceholder: 'Search maturities…',
          customValue: strictValue,
        },
      },
      {
        key: 'score_confidence',
        label: 'Score confidence',
        icon: CheckListIcon,
        placeholder: 'Select a confidence',
        picker: {
          source: enumOptions(['low', 'medium', 'high']),
          searchPlaceholder: 'Search confidence levels…',
          customValue: strictValue,
        },
      },
      {
        key: 'minimum_coverage_percent',
        label: 'Minimum coverage',
        icon: DashboardSpeed01Icon,
        placeholder: 'Select a coverage',
        picker: {
          source: coverageOptions,
          searchPlaceholder: 'Search or type a percentage…',
          customValue: customCoveragePercent,
          formatValue: (percent) => `${percent}%`,
        },
      },
    ],
  },
  {
    label: 'Owner',
    fields: [
      {
        key: 'user_id',
        label: 'User ID',
        icon: UserIcon,
        placeholder: 'user ID',
      },
      {
        key: 'team_id',
        label: 'Team ID',
        icon: UserGroupIcon,
        placeholder: 'team ID',
      },
      {
        key: 'service_account_id',
        label: 'Service account ID',
        icon: UserAccountIcon,
        placeholder: 'service account ID',
      },
    ],
  },
  {
    label: 'Source',
    fields: [
      {
        key: 'session_source_id',
        label: 'Session source ID',
        icon: HashIcon,
        placeholder: 'source ID',
      },
      {
        key: 'session_source_hash',
        label: 'Session source hash',
        icon: FingerPrintIcon,
        placeholder: 'sha256:…',
      },
      {
        key: 'request_tag',
        label: 'Request tag',
        icon: Tag01Icon,
        placeholder: 'value',
      },
    ],
  },
  {
    label: 'Start date',
    fields: [
      {
        key: 'started_after',
        label: 'Started after',
        icon: CalendarAdd01Icon,
        placeholder: 'Select a day',
        picker: dayPicker('start'),
      },
      {
        key: 'started_before',
        label: 'Started before',
        icon: Calendar04Icon,
        placeholder: 'Select a day',
        picker: dayPicker('end'),
      },
    ],
  },
]

export const sessionFilterFields: SessionFilterField[] = sessionFilterGroups.flatMap(
  (group) => group.fields,
)

export function fieldParams(key: SessionFilterFieldKey): SessionFilterParam[] {
  return key === 'request_tag' ? ['request_tag_key', 'request_tag_value'] : [key]
}

export const sessionFilterParams: SessionFilterParam[] = sessionFilterFields.flatMap((field) =>
  fieldParams(field.key),
)

/** A URL patch that removes every filter, including the search, and keeps paging. */
export const clearedSessionFilters: Partial<Record<SessionFilterParam, undefined>> =
  Object.fromEntries([...sessionFilterParams, 'q'].map((param) => [param, undefined]))

export function hasSessionFilters(values: SessionFilterValues) {
  return [...sessionFilterParams, 'q' as const].some((param) => {
    const value = values[param]
    return value !== undefined && value !== null && String(value).trim() !== ''
  })
}
