import { useEffect, useMemo, useState, useTransition } from 'react'
import {
  Calendar04Icon,
  RefreshIcon,
  RoboticIcon,
  TaskDaily01Icon,
  UserIcon,
} from '@hugeicons/core-free-icons'
import { HugeiconsIcon } from '@hugeicons/react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import {
  DateSelector,
  formatDateValue,
  type DateSelectorValue,
} from '@/components/reui/date-selector'
import {
  createFilter,
  Filters,
  type Filter,
  type FilterFieldConfig,
} from '@/components/reui/filters'
import { PageHeader } from '@/components/layout/page-header'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  BatchDetailSheet,
  BatchList,
  formatStatus,
  isBatchActive,
} from '@/routes/batches/-components'
import {
  cancelGatewayBatch,
  getBatchResultPage,
  getBatches,
  getServiceAccounts,
  getUsers,
} from '@/server/admin-data.functions'
import type {
  BatchFiltersInput,
  BatchResultsView,
  BatchStatus,
  BatchView,
  ServiceAccountView,
  UserView,
} from '@/types/api'

const defaultPageSize = 30
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

type BatchFilterValue = string | DateSelectorValue

export const Route = createFileRoute('/batches')({
  validateSearch: (search: Record<string, unknown>) => normalizeBatchSearch(search),
  loaderDeps: ({ search }) => search,
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: async ({ deps }) => {
    const [batchPage, users, serviceAccounts] = await Promise.all([
      getBatches({ data: deps }),
      getUsers(),
      getServiceAccounts(),
    ])
    return {
      batchPage,
      users: users.data.users,
      serviceAccounts: serviceAccounts.data.service_accounts,
    }
  },
  component: BatchesPage,
})

export function BatchesPage() {
  const { batchPage, users, serviceAccounts } = Route.useLoaderData() as {
    batchPage: Awaited<ReturnType<typeof getBatches>>
    users: UserView[]
    serviceAccounts: ServiceAccountView[]
  }
  const search = Route.useSearch()
  const router = useRouter()
  const [selectedBatch, setSelectedBatch] = useState<BatchView | null>(null)
  const [batchDetail, setBatchDetail] = useState<BatchResultsView | null>(null)
  const [detailPending, setDetailPending] = useState(false)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [cancelTarget, setCancelTarget] = useState<BatchView | null>(null)
  const [cancelPending, setCancelPending] = useState(false)
  const [isListPending, startListTransition] = useTransition()
  const hasActiveBatches = batchPage.items.some((batch) => isBatchActive(batch.status))
  const totalPages = Math.max(1, Math.ceil(batchPage.total / batchPage.page_size))

  useEffect(() => {
    if (!hasActiveBatches) return
    const intervalId = window.setInterval(() => {
      if (document.visibilityState === 'visible') {
        void router.invalidate()
      }
    }, 5_000)
    return () => window.clearInterval(intervalId)
  }, [hasActiveBatches, router])

  const fields = useMemo<FilterFieldConfig<BatchFilterValue>[]>(
    () => [
      {
        key: 'created_at',
        label: 'Created',
        icon: <HugeiconsIcon icon={Calendar04Icon} />,
        type: 'custom',
        defaultOperator: 'between',
        operators: [{ value: 'between', label: 'between' }],
        customRenderer: ({ values, onChange }) => (
          <DateRangeFilter values={values} onChange={onChange} />
        ),
      },
      {
        key: 'user_id',
        label: 'User',
        icon: <HugeiconsIcon icon={UserIcon} />,
        type: 'select',
        searchable: true,
        options: users.map((user) => ({
          value: user.id,
          label: `${user.name} (${user.email})`,
        })),
      },
      {
        key: 'service_account_id',
        label: 'Service account',
        icon: <HugeiconsIcon icon={RoboticIcon} />,
        type: 'select',
        searchable: true,
        options: serviceAccounts.map((account) => ({
          value: account.id,
          label: account.name,
        })),
      },
      {
        key: 'status',
        label: 'Status',
        icon: <HugeiconsIcon icon={TaskDaily01Icon} />,
        type: 'select',
        searchable: false,
        options: batchStatuses.map((status) => ({
          value: status,
          label: formatStatus(status),
        })),
      },
    ],
    [serviceAccounts, users],
  )

  const urlFilters = useMemo(
    () => batchFiltersFromSearch(search),
    [
      search.created_at_end,
      search.created_at_start,
      search.service_account_id,
      search.status,
      search.user_id,
    ],
  )
  const [filters, setFilters] = useState(urlFilters)

  useEffect(() => setFilters(urlFilters), [urlFilters])

  function applyFilters(nextFilters: Filter<BatchFilterValue>[]) {
    setFilters(nextFilters)
    const hasIncompleteDate = nextFilters.some(
      (filter) => filter.field === 'created_at' && !isDateSelectorValue(filter.values[0]),
    )
    if (hasIncompleteDate) return

    startListTransition(async () => {
      await router.navigate({
        to: '/batches',
        search: filtersToSearch(nextFilters),
      })
    })
  }

  function navigateToPage(page: number) {
    startListTransition(async () => {
      await router.navigate({
        to: '/batches',
        search: { ...search, page },
      })
    })
  }

  async function inspectBatch(batch: BatchView) {
    setSelectedBatch(batch)
    setBatchDetail(null)
    setDetailError(null)
    setDetailPending(true)
    try {
      const detail = await getBatchResultPage({ data: { batchId: batch.batch_id } })
      setBatchDetail(detail)
    } catch (error: unknown) {
      setDetailError(error instanceof Error ? error.message : 'Failed to load batch responses')
    } finally {
      setDetailPending(false)
    }
  }

  async function cancelBatch() {
    if (!cancelTarget) return
    setCancelPending(true)
    try {
      await cancelGatewayBatch({ data: { batchId: cancelTarget.batch_id } })
      toast.success('Batch cancellation requested')
      setCancelTarget(null)
      await router.invalidate()
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : 'Batch cancellation failed')
    } finally {
      setCancelPending(false)
    }
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Control Plane"
        title="Batch requests"
        description="Track long-running model jobs, inspect normalized responses, and cancel work that has not finished."
        actions={
          <Button
            type="button"
            variant="outline"
            disabled={isListPending}
            onClick={() => void router.invalidate()}
          >
            <HugeiconsIcon icon={RefreshIcon} data-icon="inline-start" />
            Refresh
          </Button>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle>Batch list</CardTitle>
          <CardDescription>
            Filters are applied by the gateway. Active batches refresh every five seconds while this
            page is visible.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex min-h-9 flex-wrap items-center gap-2" aria-busy={isListPending}>
            <Filters
              filters={filters}
              fields={fields}
              onChange={applyFilters}
              allowMultiple={false}
              size="sm"
            />
            {filters.length > 0 ? (
              <Button type="button" variant="ghost" size="sm" onClick={() => applyFilters([])}>
                Clear all
              </Button>
            ) : null}
          </div>
          <div className="text-muted-foreground flex flex-wrap items-center justify-between gap-3 text-sm">
            <span>
              {batchPage.total} {batchPage.total === 1 ? 'batch' : 'batches'} in the current scope
            </span>
            {isListPending ? (
              <span role="status" aria-live="polite">
                Updating list...
              </span>
            ) : null}
          </div>
          <BatchList
            batches={batchPage.items}
            onInspect={(batch) => void inspectBatch(batch)}
            onCancel={setCancelTarget}
          />
          {batchPage.total > batchPage.page_size ? (
            <div className="flex flex-wrap items-center justify-between gap-3 border-t pt-4">
              <p className="text-muted-foreground text-sm">
                Page {batchPage.page} of {totalPages}
              </p>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={batchPage.page <= 1 || isListPending}
                  onClick={() => navigateToPage(batchPage.page - 1)}
                >
                  Previous
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={batchPage.page >= totalPages || isListPending}
                  onClick={() => navigateToPage(batchPage.page + 1)}
                >
                  Next
                </Button>
              </div>
            </div>
          ) : null}
        </CardContent>
      </Card>

      <BatchDetailSheet
        batch={selectedBatch}
        detail={batchDetail}
        pending={detailPending}
        error={detailError}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedBatch(null)
            setBatchDetail(null)
            setDetailError(null)
          }
        }}
      />

      <AlertDialog
        open={cancelTarget !== null}
        onOpenChange={(open) => !open && setCancelTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Cancel this batch?</AlertDialogTitle>
            <AlertDialogDescription>
              Queued work stops immediately. Submitted work asks the provider to cancel and can
              continue until the provider confirms the request.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={cancelPending}>Keep batch</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={cancelPending}
              onClick={(event) => {
                event.preventDefault()
                void cancelBatch()
              }}
            >
              {cancelPending ? 'Cancelling...' : 'Cancel batch'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function DateRangeFilter({
  values,
  onChange,
}: {
  values: BatchFilterValue[]
  onChange: (values: BatchFilterValue[]) => void
}) {
  const current = values.find(isDateSelectorValue)
  const [draft, setDraft] = useState<DateSelectorValue | undefined>(current)
  const [open, setOpen] = useState(false)
  const label = current ? formatDateValue(current, undefined, 'MMM dd, yyyy') : 'Select dates'

  return (
    <Popover
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen)
        if (nextOpen) setDraft(current)
      }}
    >
      <PopoverTrigger className="cursor-pointer text-sm">{label}</PopoverTrigger>
      <PopoverContent align="start" className="w-auto max-w-[calc(100vw-2rem)] p-4">
        <DateSelector
          value={draft}
          onChange={setDraft}
          periodTypes={['day']}
          presetMode="between"
          showInput={false}
          showTwoMonths
          minYear={2020}
          maxYear={new Date().getFullYear() + 1}
        />
        <div className="mt-4 flex items-center justify-end gap-2 border-t pt-3">
          <Button type="button" variant="outline" size="sm" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={!draft?.startDate}
            onClick={() => {
              if (draft?.startDate) onChange([draft])
              setOpen(false)
            }}
          >
            Apply
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function batchFiltersFromSearch(search: BatchFiltersInput): Filter<BatchFilterValue>[] {
  const filters: Filter<BatchFilterValue>[] = []
  const dateValue = dateValueFromSearch(search)
  if (dateValue) filters.push(createFilter('created_at', 'between', [dateValue]))
  if (search.user_id) filters.push(createFilter('user_id', 'is', [search.user_id]))
  if (search.service_account_id) {
    filters.push(createFilter('service_account_id', 'is', [search.service_account_id]))
  }
  if (search.status) filters.push(createFilter('status', 'is', [search.status]))
  return filters
}

function filtersToSearch(filters: Filter<BatchFilterValue>[]): BatchFiltersInput {
  const search: BatchFiltersInput = { page: 1, page_size: defaultPageSize }
  for (const filter of filters) {
    const value = filter.values[0]
    if (filter.field === 'created_at' && isDateSelectorValue(value) && value.startDate) {
      search.created_at_start = startOfDayIso(value.startDate)
      search.created_at_end = exclusiveEndIso(value.endDate ?? value.startDate)
    } else if (typeof value === 'string') {
      if (filter.field === 'user_id') search.user_id = value
      if (filter.field === 'service_account_id') search.service_account_id = value
      if (filter.field === 'status' && isBatchStatus(value)) search.status = value
    }
  }
  return search
}

function normalizeBatchSearch(search: Record<string, unknown>): BatchFiltersInput {
  const page = positiveInteger(search.page, 1)
  const normalized: BatchFiltersInput = { page, page_size: defaultPageSize }
  if (typeof search.status === 'string' && isBatchStatus(search.status)) {
    normalized.status = search.status
  }
  if (typeof search.user_id === 'string' && search.user_id) normalized.user_id = search.user_id
  if (typeof search.service_account_id === 'string' && search.service_account_id) {
    normalized.service_account_id = search.service_account_id
  }
  if (isValidDateString(search.created_at_start)) {
    normalized.created_at_start = search.created_at_start
  }
  if (isValidDateString(search.created_at_end)) normalized.created_at_end = search.created_at_end
  return normalized
}

function dateValueFromSearch(search: BatchFiltersInput): DateSelectorValue | undefined {
  if (!search.created_at_start) return undefined
  const startDate = new Date(search.created_at_start)
  const endDate = search.created_at_end
    ? new Date(new Date(search.created_at_end).getTime() - 1)
    : startDate
  return { period: 'day', operator: 'between', startDate, endDate }
}

function isDateSelectorValue(value: BatchFilterValue | undefined): value is DateSelectorValue {
  return typeof value === 'object' && value !== null && 'period' in value
}

function isBatchStatus(value: string): value is BatchStatus {
  return batchStatuses.includes(value as BatchStatus)
}

function positiveInteger(value: unknown, fallback: number) {
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback
}

function isValidDateString(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && !Number.isNaN(new Date(value).getTime())
}

function startOfDayIso(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).toISOString()
}

function exclusiveEndIso(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).toISOString()
}
