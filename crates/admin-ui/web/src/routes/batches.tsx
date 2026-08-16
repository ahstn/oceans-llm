import { useEffect, useState, useTransition } from 'react'
import { RefreshIcon } from '@hugeicons/core-free-icons'
import { HugeiconsIcon } from '@hugeicons/react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

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
import { isPlatformAdminSession } from '@/routes/-auth-routing'
import { BatchFilterBar } from '@/routes/batches/-filter-bar'
import { BatchDetailSheet, BatchList, isBatchActive } from '@/routes/batches/-components'
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
const resultPageSize = 100
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

export const Route = createFileRoute('/batches')({
  validateSearch: (search: Record<string, unknown>) => normalizeBatchSearch(search),
  loaderDeps: ({ search }) => search,
  loader: async ({ context, deps }) => {
    const canFilterAcrossUsers = isPlatformAdminSession(context.session)
    const [batchPage, users, serviceAccounts] = await Promise.all([
      getBatches({ data: deps }),
      canFilterAcrossUsers ? getUsers() : null,
      canFilterAcrossUsers ? getServiceAccounts() : null,
    ])
    return {
      batchPage,
      users: users?.data.users ?? [],
      serviceAccounts: serviceAccounts?.data.service_accounts ?? [],
    }
  },
  component: BatchesPage,
})

export function BatchesPage() {
  const { session } = Route.useRouteContext()
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

  function applyFilters(nextFilters: BatchFiltersInput) {
    startListTransition(async () => {
      await router.navigate({
        to: '/batches',
        search: nextFilters,
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
    await loadBatchResults(batch, 1)
  }

  async function loadBatchResults(batch: BatchView, page: number) {
    setBatchDetail(null)
    setDetailError(null)
    setDetailPending(true)
    try {
      const detail = await getBatchResultPage({
        data: { batchId: batch.batch_id, page, pageSize: resultPageSize },
      })
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
          <BatchFilterBar
            key={filterStateKey(search)}
            initialFilters={search}
            users={users}
            serviceAccounts={serviceAccounts}
            canFilterAcrossUsers={isPlatformAdminSession(session)}
            pending={isListPending}
            onApply={applyFilters}
          />
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
        onPageChange={(page) => {
          if (selectedBatch) void loadBatchResults(selectedBatch, page)
        }}
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

function filterStateKey(search: BatchFiltersInput) {
  return [
    search.status,
    search.user_id,
    search.service_account_id,
    search.created_at_start,
    search.created_at_end,
  ].join('|')
}
