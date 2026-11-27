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

import {
  clearedFilters,
  dayPicker,
  enumOptions,
  groupParams,
  hasFilters,
  strictValue,
  type FilterField,
  type FilterGroup,
} from '@/components/data/filter-fields'
import { anyTypedValue, type FilterOption } from '@/components/data/filter-options'
import type { AgentSessionFiltersInput } from '@/types/api'

/** Every agent session filter the URL search carries; paging stays with the table. */
export type SessionFilterValues = Omit<AgentSessionFiltersInput, 'page' | 'page_size'>
export type SessionFilterParam = keyof SessionFilterValues
type SessionFilterField = FilterField<SessionFilterParam>

/** One chip per param, except the request tag chip, which owns its key and value. */
function field(
  key: Exclude<SessionFilterParam, 'request_tag_key' | 'request_tag_value'>,
  rest: Omit<SessionFilterField, 'key' | 'params'>,
): SessionFilterField {
  return { key, params: [key], ...rest }
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

export const sessionFilterGroups: FilterGroup<SessionFilterParam>[] = [
  {
    label: 'Session',
    fields: [
      field('lifecycle', {
        label: 'State',
        icon: StatusIcon,
        placeholder: 'Select a state',
        picker: {
          source: enumOptions(['open', 'finalized']),
          searchPlaceholder: 'Search states…',
          customValue: strictValue,
        },
      }),
      field('gateway_outcome', {
        label: 'Outcome',
        icon: Flag02Icon,
        placeholder: 'Select an outcome',
        picker: {
          source: enumOptions(['succeeded', 'partial', 'failed', 'unknown']),
          searchPlaceholder: 'Search outcomes…',
          customValue: strictValue,
        },
      }),
      field('harness_key', {
        label: 'Harness',
        icon: Robot01Icon,
        placeholder: 'Select a harness',
        picker: {
          source: 'harnesses',
          searchPlaceholder: 'Search or type a harness key…',
          customValue: anyTypedValue,
        },
      }),
      field('requested_model_key', {
        label: 'Model',
        icon: AiChipIcon,
        placeholder: 'Select a model',
        picker: {
          source: 'models',
          searchPlaceholder: 'Search models…',
          customValue: anyTypedValue,
        },
      }),
      field('operation', {
        label: 'Operation',
        icon: ApiIcon,
        placeholder: 'Select an operation',
        picker: {
          source: operationOptions,
          searchPlaceholder: 'Search or type an operation…',
          customValue: anyTypedValue,
        },
      }),
      field('caller_class', {
        label: 'Caller class',
        icon: UserSquareIcon,
        placeholder: 'Select a caller class',
        picker: {
          source: callerClassOptions,
          searchPlaceholder: 'Search caller classes…',
          customValue: anyTypedValue,
        },
      }),
    ],
  },
  {
    label: 'Score',
    fields: [
      field('score_maturity', {
        label: 'Score maturity',
        icon: Target02Icon,
        placeholder: 'Select a maturity',
        picker: {
          source: enumOptions(['experimental', 'calibrated']),
          searchPlaceholder: 'Search maturities…',
          customValue: strictValue,
        },
      }),
      field('score_confidence', {
        label: 'Score confidence',
        icon: CheckListIcon,
        placeholder: 'Select a confidence',
        picker: {
          source: enumOptions(['low', 'medium', 'high']),
          searchPlaceholder: 'Search confidence levels…',
          customValue: strictValue,
        },
      }),
      field('minimum_coverage_percent', {
        label: 'Minimum coverage',
        icon: DashboardSpeed01Icon,
        placeholder: 'Select a coverage',
        picker: {
          source: coverageOptions,
          searchPlaceholder: 'Search or type a percentage…',
          customValue: customCoveragePercent,
          formatValue: (percent) => `${percent}%`,
        },
      }),
    ],
  },
  {
    label: 'Owner',
    fields: [
      field('user_id', { label: 'User ID', icon: UserIcon, placeholder: 'user ID' }),
      field('team_id', { label: 'Team ID', icon: UserGroupIcon, placeholder: 'team ID' }),
      field('service_account_id', {
        label: 'Service account ID',
        icon: UserAccountIcon,
        placeholder: 'service account ID',
      }),
    ],
  },
  {
    label: 'Source',
    fields: [
      field('session_source_id', {
        label: 'Session source ID',
        icon: HashIcon,
        placeholder: 'source ID',
      }),
      field('session_source_hash', {
        label: 'Session source hash',
        icon: FingerPrintIcon,
        placeholder: 'sha256:…',
      }),
      {
        key: 'request_tag',
        label: 'Request tag',
        icon: Tag01Icon,
        placeholder: 'value',
        params: ['request_tag_key', 'request_tag_value'],
        pair: {
          keyLabel: 'Request tag key',
          keyPlaceholder: 'key',
          valueLabel: 'Request tag value',
          incompleteTitle: 'Incomplete tag filter',
          incompleteDescription:
            'Provide both a tag key and tag value to filter sessions by a request tag.',
        },
      },
    ],
  },
  {
    label: 'Start date',
    fields: [
      field('started_after', {
        label: 'Started after',
        icon: CalendarAdd01Icon,
        placeholder: 'Select a day',
        picker: dayPicker('start'),
      }),
      field('started_before', {
        label: 'Started before',
        icon: Calendar04Icon,
        placeholder: 'Select a day',
        picker: dayPicker('end'),
      }),
    ],
  },
]

export const sessionFilterParams: SessionFilterParam[] = groupParams(sessionFilterGroups)

/** A URL patch that removes every filter and keeps paging. */
export const clearedSessionFilters = clearedFilters(sessionFilterGroups)

export function hasSessionFilters(values: SessionFilterValues) {
  return hasFilters(sessionFilterGroups, values)
}
