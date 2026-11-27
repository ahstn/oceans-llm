import { useMemo } from 'react'
import {
  Calendar04Icon,
  CalendarAdd01Icon,
  StatusIcon,
  UserAccountIcon,
  UserIcon,
} from '@hugeicons/core-free-icons'

import {
  dayPicker,
  strictValue,
  type FilterGroup,
  type FilterPatch,
} from '@/components/data/filter-fields'
import { FilterToolbar } from '@/components/data/filter-toolbar'
import { formatStatus } from '@/routes/batches/-utils'
import type { BatchFiltersInput, BatchStatus, ServiceAccountView, UserView } from '@/types/api'

const batchStatuses: BatchStatus[] = [
  'queued',
  'submitting',
  'submission_unknown',
  'validating',
  'in_progress',
  'finalizing',
  'completed',
  'failed',
  'expired',
  'cancel_requested',
  'cancelling',
  'cancelled',
]

export type BatchFilterParam =
  | 'status'
  | 'user_id'
  | 'service_account_id'
  | 'created_at_start'
  | 'created_at_end'

export type BatchFilterPatch = FilterPatch<BatchFilterParam>

const statusField = {
  key: 'status',
  params: ['status'],
  label: 'Status',
  icon: StatusIcon,
  placeholder: 'Select a status',
  picker: {
    source: batchStatuses.map((status) => ({ value: status, label: formatStatus(status) })),
    searchPlaceholder: 'Search statuses…',
    customValue: strictValue,
  },
} satisfies FilterGroup<BatchFilterParam>['fields'][number]

const dateGroup: FilterGroup<BatchFilterParam> = {
  label: 'Created',
  fields: [
    {
      key: 'created_at_start',
      params: ['created_at_start'],
      label: 'Created after',
      icon: CalendarAdd01Icon,
      placeholder: 'Select a day',
      picker: dayPicker('start'),
    },
    {
      key: 'created_at_end',
      params: ['created_at_end'],
      label: 'Created before',
      icon: Calendar04Icon,
      placeholder: 'Select a day',
      // Batch stores compare `created_at < end` on whole seconds, so send the next midnight.
      picker: dayPicker('end-exclusive'),
    },
  ],
}

/** The batch filter groups. Owner fields list the loaded users and service accounts. */
function batchFilterGroups(
  users: UserView[],
  serviceAccounts: ServiceAccountView[],
  canFilterAcrossUsers: boolean,
): FilterGroup<BatchFilterParam>[] {
  const ownerGroup: FilterGroup<BatchFilterParam> = {
    label: 'Owner',
    fields: [
      {
        key: 'user_id',
        params: ['user_id'],
        label: 'User',
        icon: UserIcon,
        placeholder: 'Select a user',
        picker: {
          source: users.map((user) => ({ value: user.id, label: user.name, hint: user.email })),
          searchPlaceholder: 'Search users…',
          customValue: strictValue,
        },
      },
      {
        key: 'service_account_id',
        params: ['service_account_id'],
        label: 'Service account',
        icon: UserAccountIcon,
        placeholder: 'Select a service account',
        picker: {
          source: serviceAccounts.map((account) => ({ value: account.id, label: account.name })),
          searchPlaceholder: 'Search service accounts…',
          customValue: strictValue,
        },
      },
    ],
  }
  return [
    { label: 'Batch', fields: [statusField] },
    ...(canFilterAcrossUsers ? [ownerGroup] : []),
    dateGroup,
  ]
}

/** DESIGN.md's filter toolbar over the gateway's batch filters. Each change applies at once. */
export function BatchFilterBar({
  filters,
  users,
  serviceAccounts,
  canFilterAcrossUsers,
  shownCount,
  totalCount,
  pending,
  onApply,
}: {
  filters: BatchFiltersInput
  users: UserView[]
  serviceAccounts: ServiceAccountView[]
  canFilterAcrossUsers: boolean
  shownCount: number
  totalCount: number
  pending: boolean
  onApply: (patch: BatchFilterPatch) => void
}) {
  const groups = useMemo(
    () => batchFilterGroups(users, serviceAccounts, canFilterAcrossUsers),
    [users, serviceAccounts, canFilterAcrossUsers],
  )
  return (
    <FilterToolbar
      groups={groups}
      filters={filters}
      shownCount={shownCount}
      totalCount={totalCount}
      isPending={pending}
      testIdPrefix="batch"
      onApply={onApply}
    />
  )
}
