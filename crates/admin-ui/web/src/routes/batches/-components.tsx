import { CancelCircleIcon, DatabaseSearchIcon, FileViewIcon } from '@hugeicons/core-free-icons'
import { HugeiconsIcon } from '@hugeicons/react'
import { format } from 'date-fns'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import type { BatchResultView, BatchResultsView, BatchStatus, BatchView } from '@/types/api'

const currencyFormatter = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
})

export function BatchList({
  batches,
  onInspect,
  onCancel,
}: {
  batches: BatchView[]
  onInspect: (batch: BatchView) => void
  onCancel: (batch: BatchView) => void
}) {
  if (batches.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <AppIcon icon={DatabaseSearchIcon} size={22} stroke={1.5} />
          </EmptyMedia>
          <EmptyTitle>No batch requests found</EmptyTitle>
          <EmptyDescription>
            No batch requests match the current filters. Clear a filter or submit a batch through
            the gateway API.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:hidden" data-testid="batch-mobile-list">
        {batches.map((batch) => (
          <article key={batch.batch_id} className="bg-muted/40 rounded-lg border p-4">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <p className="truncate font-semibold">{batch.model}</p>
                <p className="text-muted-foreground truncate font-mono text-xs">{batch.batch_id}</p>
              </div>
              <BatchStatusBadge status={batch.status} />
            </div>
            <dl className="mt-4 grid grid-cols-2 gap-3 text-sm">
              <SummaryField label="Provider" value={batch.provider} />
              <SummaryField label="Caller" value={callerLabel(batch)} />
              <SummaryField label="Progress" value={progressLabel(batch)} />
              <SummaryField label="Created" value={formatDateTime(batch.created_at)} />
            </dl>
            <div className="mt-4 flex flex-wrap gap-2">
              <Button type="button" variant="outline" size="sm" onClick={() => onInspect(batch)}>
                <HugeiconsIcon icon={FileViewIcon} data-icon="inline-start" />
                View responses
              </Button>
              {isBatchCancellable(batch.status) ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  onClick={() => onCancel(batch)}
                >
                  <HugeiconsIcon icon={CancelCircleIcon} data-icon="inline-start" />
                  Cancel
                </Button>
              ) : null}
            </div>
          </article>
        ))}
      </div>

      <div
        className="hidden overflow-hidden rounded-md border md:block"
        data-testid="batch-desktop-table"
      >
        <Table className="min-w-[70rem] text-left">
          <TableHeader className="bg-muted/60">
            <TableRow>
              <TableHead className="px-3 py-2 font-semibold">Created</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Model</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Provider</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Caller</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Status</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Progress</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Cost</TableHead>
              <TableHead className="px-3 py-2 font-semibold">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {batches.map((batch) => (
              <TableRow key={batch.batch_id}>
                <TableCell className="px-3 py-3 whitespace-nowrap">
                  {formatDateTime(batch.created_at)}
                </TableCell>
                <TableCell className="px-3 py-3">
                  <div className="min-w-0">
                    <p className="truncate font-semibold">{batch.model}</p>
                    <p className="text-muted-foreground truncate font-mono text-xs">
                      {batch.batch_id}
                    </p>
                  </div>
                </TableCell>
                <TableCell className="px-3 py-3">
                  <p>{batch.provider}</p>
                  <p className="text-muted-foreground text-xs">{formatEndpoint(batch.endpoint)}</p>
                </TableCell>
                <TableCell className="px-3 py-3">
                  <p className="font-medium">{callerLabel(batch)}</p>
                  <p className="text-muted-foreground text-xs">{callerKind(batch)}</p>
                </TableCell>
                <TableCell className="px-3 py-3">
                  <BatchStatusBadge status={batch.status} />
                </TableCell>
                <TableCell className="px-3 py-3 tabular-nums">{progressLabel(batch)}</TableCell>
                <TableCell className="px-3 py-3 tabular-nums">
                  {batch.cost_usd === null ? 'Pending' : currencyFormatter.format(batch.cost_usd)}
                </TableCell>
                <TableCell className="px-3 py-3">
                  <div className="flex items-center gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => onInspect(batch)}
                    >
                      <HugeiconsIcon icon={FileViewIcon} data-icon="inline-start" />
                      View
                    </Button>
                    {isBatchCancellable(batch.status) ? (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="text-destructive hover:text-destructive"
                        onClick={() => onCancel(batch)}
                      >
                        <HugeiconsIcon icon={CancelCircleIcon} data-icon="inline-start" />
                        Cancel
                      </Button>
                    ) : null}
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

export function BatchDetailSheet({
  batch,
  detail,
  pending,
  error,
  onOpenChange,
}: {
  batch: BatchView | null
  detail: BatchResultsView | null
  pending: boolean
  error: string | null
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Sheet open={batch !== null} onOpenChange={onOpenChange}>
      <SheetContent className="max-w-full min-w-0 overflow-hidden sm:max-w-3xl">
        <SheetHeader className="border-b pr-12">
          <SheetTitle>Batch responses</SheetTitle>
          <SheetDescription>
            Review the batch summary and each normalized gateway result.
          </SheetDescription>
        </SheetHeader>
        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto px-4 pb-4">
          {batch ? <BatchSummary batch={detail?.batch ?? batch} /> : null}
          {pending ? <BatchDetailSkeleton /> : null}
          {error ? (
            <Alert variant="destructive" role="status" aria-live="polite" className="mt-4">
              <AlertTitle>Responses could not be loaded</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          {detail ? <BatchResults results={detail.items} total={detail.total} /> : null}
        </div>
      </SheetContent>
    </Sheet>
  )
}

function BatchSummary({ batch }: { batch: BatchView }) {
  return (
    <dl className="bg-muted/40 mt-4 grid gap-4 rounded-lg border p-4 sm:grid-cols-2">
      <SummaryField label="Batch ID" value={batch.batch_id} mono />
      <SummaryField label="Status" value={formatStatus(batch.status)} />
      <SummaryField label="Model" value={batch.model} />
      <SummaryField label="Provider" value={batch.provider} />
      <SummaryField label="Caller" value={callerLabel(batch)} />
      <SummaryField label="Progress" value={progressLabel(batch)} />
      <SummaryField label="Created" value={formatDateTime(batch.created_at)} />
      <SummaryField
        label="Cost"
        value={batch.cost_usd === null ? 'Pending' : currencyFormatter.format(batch.cost_usd)}
      />
    </dl>
  )
}

function BatchResults({ results, total }: { results: BatchResultView[]; total: number }) {
  if (results.length === 0) {
    return (
      <Empty className="mt-4 border">
        <EmptyHeader>
          <EmptyTitle>No responses available</EmptyTitle>
          <EmptyDescription>
            This batch has not produced any normalized responses yet. Active batches refresh in the
            list every five seconds.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <section className="mt-6 flex min-w-0 flex-col gap-3">
      <div>
        <h2 className="font-semibold">Results</h2>
        <p className="text-muted-foreground text-sm">
          Showing {results.length} of {total} responses.
        </p>
      </div>
      {results.map((result) => (
        <article key={result.custom_id} className="min-w-0 overflow-hidden rounded-lg border">
          <div className="bg-muted/50 flex flex-wrap items-center justify-between gap-2 border-b px-3 py-2">
            <span className="font-mono text-sm font-semibold">{result.custom_id}</span>
            <Badge variant={result.status === 'succeeded' ? 'success' : 'destructive'}>
              {formatStatus(result.status)}
            </Badge>
          </div>
          <div className="grid min-w-0 gap-4 p-3">
            <JsonBlock
              label={result.error === null ? 'Response' : 'Error'}
              value={result.error ?? result.response}
            />
            <details className="group min-w-0">
              <summary className="text-muted-foreground cursor-pointer text-sm font-medium">
                Request payload
              </summary>
              <JsonBlock value={result.request} />
            </details>
          </div>
        </article>
      ))}
    </section>
  )
}

function JsonBlock({ label, value }: { label?: string; value: unknown }) {
  return (
    <div className="max-w-full min-w-0 overflow-hidden">
      {label ? <p className="mb-1 text-sm font-medium">{label}</p> : null}
      <pre className="bg-muted/50 max-w-full overflow-x-auto rounded-md border p-3 font-mono text-xs leading-relaxed">
        {value === null ? 'No payload available' : JSON.stringify(value, null, 2)}
      </pre>
    </div>
  )
}

function BatchDetailSkeleton() {
  return (
    <div className="mt-6 flex flex-col gap-3" role="status" aria-live="polite">
      <span className="sr-only">Loading batch responses</span>
      <Skeleton className="h-5 w-32" />
      <Skeleton className="h-32 w-full" />
      <Skeleton className="h-32 w-full" />
    </div>
  )
}

function SummaryField({
  label,
  value,
  mono = false,
}: {
  label: string
  value: string
  mono?: boolean
}) {
  return (
    <div className="min-w-0">
      <dt className="text-muted-foreground text-xs font-semibold tracking-wide uppercase">
        {label}
      </dt>
      <dd className={`mt-1 truncate ${mono ? 'font-mono text-xs' : ''}`}>{value}</dd>
    </div>
  )
}

export function BatchStatusBadge({ status }: { status: BatchStatus }) {
  const variant = statusVariant(status)
  return <Badge variant={variant}>{formatStatus(status)}</Badge>
}

function statusVariant(status: BatchStatus): 'success' | 'warning' | 'destructive' | 'secondary' {
  if (status === 'completed') return 'success'
  if (status === 'failed' || status === 'expired' || status === 'submission_unknown') {
    return 'destructive'
  }
  if (status === 'cancelled') return 'secondary'
  return 'warning'
}

export function isBatchActive(status: BatchStatus) {
  return !['completed', 'failed', 'expired', 'cancelled', 'submission_unknown'].includes(status)
}

export function isBatchCancellable(status: BatchStatus) {
  return ['queued', 'validating', 'in_progress', 'finalizing'].includes(status)
}

export function formatStatus(status: string) {
  return status
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ')
}

function formatEndpoint(endpoint: BatchView['endpoint']) {
  return formatStatus(endpoint)
}

function progressLabel(batch: BatchView) {
  const done = Math.min(batch.request_count, batch.completed_count + batch.failed_count)
  return `${done} of ${batch.request_count}`
}

function callerLabel(batch: BatchView) {
  return (
    batch.caller.service_account_name ??
    batch.caller.user_name ??
    batch.caller.api_key_name ??
    batch.caller.api_key_id
  )
}

function callerKind(batch: BatchView) {
  if (batch.caller.service_account_id) return 'Service account'
  if (batch.caller.user_id) return 'User'
  return 'API key'
}

function formatDateTime(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return format(date, 'MMM dd, yyyy, HH:mm')
}
