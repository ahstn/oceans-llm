import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { formatStatus } from '@/routes/batches/-components'
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

interface BatchFilterDraft {
  status: BatchStatus | 'all'
  userId: string
  serviceAccountId: string
  startDate: string
  endDate: string
}

export function BatchFilterBar({
  initialFilters,
  users,
  serviceAccounts,
  canFilterAcrossUsers,
  pending,
  onApply,
}: {
  initialFilters: BatchFiltersInput
  users: UserView[]
  serviceAccounts: ServiceAccountView[]
  canFilterAcrossUsers: boolean
  pending: boolean
  onApply: (filters: BatchFiltersInput) => void
}) {
  const [draft, setDraft] = useState<BatchFilterDraft>(() => draftFromFilters(initialFilters))
  const invalidDateRange = Boolean(
    (draft.endDate && !draft.startDate) ||
    (draft.startDate && draft.endDate && draft.endDate < draft.startDate),
  )

  function update<K extends keyof BatchFilterDraft>(key: K, value: BatchFilterDraft[K]) {
    setDraft((current) => ({ ...current, [key]: value }))
  }

  return (
    <div className="flex flex-col gap-3" aria-busy={pending}>
      <FieldGroup className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Field>
          <FieldLabel htmlFor="batch-filter-start">Created from</FieldLabel>
          <Input
            id="batch-filter-start"
            type="date"
            value={draft.startDate}
            onChange={(event) => update('startDate', event.target.value)}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="batch-filter-end">Created through</FieldLabel>
          <Input
            id="batch-filter-end"
            type="date"
            min={draft.startDate || undefined}
            value={draft.endDate}
            onChange={(event) => update('endDate', event.target.value)}
          />
        </Field>
        <Field>
          <FieldLabel>Status</FieldLabel>
          <Select
            value={draft.status}
            onValueChange={(value) => update('status', value as BatchFilterDraft['status'])}
          >
            <SelectTrigger className="w-full" data-testid="batch-filter-status">
              <SelectValue placeholder="All statuses" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All statuses</SelectItem>
                {batchStatuses.map((status) => (
                  <SelectItem key={status} value={status}>
                    {formatStatus(status)}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
        {canFilterAcrossUsers ? (
          <Field>
            <FieldLabel>User</FieldLabel>
            <Select
              value={draft.userId || 'all'}
              onValueChange={(value) => update('userId', value === 'all' ? '' : value)}
            >
              <SelectTrigger className="w-full" data-testid="batch-filter-user">
                <SelectValue placeholder="All users" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="all">All users</SelectItem>
                  {users.map((user) => (
                    <SelectItem key={user.id} value={user.id}>
                      {user.name} ({user.email})
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
        ) : null}
        {canFilterAcrossUsers ? (
          <Field>
            <FieldLabel>Service account</FieldLabel>
            <Select
              value={draft.serviceAccountId || 'all'}
              onValueChange={(value) => update('serviceAccountId', value === 'all' ? '' : value)}
            >
              <SelectTrigger className="w-full" data-testid="batch-filter-service-account">
                <SelectValue placeholder="All service accounts" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="all">All service accounts</SelectItem>
                  {serviceAccounts.map((account) => (
                    <SelectItem key={account.id} value={account.id}>
                      {account.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
        ) : null}
      </FieldGroup>

      {invalidDateRange ? (
        <p className="text-destructive text-sm" role="alert">
          Select a start date that is on or before the end date.
        </p>
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="secondary"
          disabled={pending || invalidDateRange}
          onClick={() => onApply(filtersFromDraft(draft, initialFilters.page_size))}
        >
          {pending ? 'Filtering...' : 'Apply filters'}
        </Button>
        <Button
          type="button"
          variant="ghost"
          disabled={pending}
          onClick={() => onApply({ page: 1, page_size: initialFilters.page_size })}
        >
          Clear
        </Button>
      </div>
    </div>
  )
}

function draftFromFilters(filters: BatchFiltersInput): BatchFilterDraft {
  return {
    status: filters.status ?? 'all',
    userId: filters.user_id ?? '',
    serviceAccountId: filters.service_account_id ?? '',
    startDate: dateInputValue(filters.created_at_start),
    endDate: dateInputValue(filters.created_at_end, true),
  }
}

function filtersFromDraft(
  draft: BatchFilterDraft,
  pageSize: number | undefined,
): BatchFiltersInput {
  const filters: BatchFiltersInput = { page: 1, page_size: pageSize }
  if (draft.status !== 'all') filters.status = draft.status
  if (draft.userId) filters.user_id = draft.userId
  if (draft.serviceAccountId) filters.service_account_id = draft.serviceAccountId
  if (draft.startDate) {
    filters.created_at_start = localDayStart(draft.startDate).toISOString()
    filters.created_at_end = exclusiveLocalDayEnd(draft.endDate || draft.startDate).toISOString()
  }
  return filters
}

function dateInputValue(value: string | undefined, exclusiveEnd = false) {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  if (exclusiveEnd) date.setDate(date.getDate() - 1)
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function localDayStart(value: string) {
  return new Date(`${value}T00:00:00`)
}

function exclusiveLocalDayEnd(value: string) {
  const date = localDayStart(value)
  date.setDate(date.getDate() + 1)
  return date
}
