import { useQuery } from '@tanstack/react-query'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Skeleton } from '@/components/ui/skeleton'
import { getAuditLogEntry } from '@/server/admin-data.functions'
import type { AuditLogSummaryView } from '@/types/api'

import {
  actionVariant,
  actionVerbLabel,
  actorKindLabel,
  actorLabel,
  formatAuditTimestamp,
  targetTypeLabel,
} from './audit-meta'

/**
 * Read-only detail for one entry. The list carries no summaries, so the redacted
 * before, after and details JSON load when the sheet opens.
 */
export function AuditEntrySheet({
  entry,
  onOpenChange,
}: {
  entry: AuditLogSummaryView | null
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Sheet open={entry !== null} onOpenChange={onOpenChange}>
      <SheetContent className="overflow-y-auto sm:max-w-lg" data-testid="audit-entry-sheet">
        {entry ? (
          <>
            <SheetHeader>
              <SheetTitle className="flex items-center gap-2">
                {targetTypeLabel(entry.target_type)}
                <Badge variant={actionVariant(entry.action)}>{actionVerbLabel(entry.action)}</Badge>
              </SheetTitle>
              <SheetDescription>{formatAuditTimestamp(entry.occurred_at)}</SheetDescription>
            </SheetHeader>
            <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-3 px-4 text-sm">
              <Fact label="Action">
                <Mono>{entry.action}</Mono>
              </Fact>
              <Fact label="Target">{entry.target_label ?? '—'}</Fact>
              <Fact label="Target ID">
                <Mono>{entry.target_id ?? '—'}</Mono>
              </Fact>
              <Fact label="Actor">{actorLabel(entry)}</Fact>
              <Fact label="Actor type">{actorKindLabel(entry.actor.kind)}</Fact>
              {entry.actor.user_id ? (
                <Fact label="Actor ID">
                  <Mono>{entry.actor.user_id}</Mono>
                </Fact>
              ) : null}
              {entry.actor.api_key_id ? (
                <Fact label="API key ID">
                  <Mono>{entry.actor.api_key_id}</Mono>
                </Fact>
              ) : null}
            </dl>
            <Separator />
            <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-3 px-4 text-sm">
              <Fact label="IP address">
                <Mono>{entry.request_ip ?? '—'}</Mono>
              </Fact>
              <Fact label="User agent">
                <span className="text-xs break-all">{entry.user_agent ?? '—'}</span>
              </Fact>
            </dl>
            <Separator />
            <EntrySummaries entryId={entry.id} />
          </>
        ) : null}
      </SheetContent>
    </Sheet>
  )
}

function EntrySummaries({ entryId }: { entryId: string }) {
  const detail = useQuery({
    queryKey: ['audit-log-entry', entryId],
    queryFn: () => getAuditLogEntry({ data: { id: entryId } }),
    retry: false,
  })
  if (detail.isPending) {
    return (
      <div className="flex flex-col gap-3 px-4 pb-4" aria-busy="true">
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
      </div>
    )
  }
  if (detail.isError) {
    return (
      <div className="px-4 pb-4">
        <Alert variant="destructive">
          <AlertDescription>
            {detail.error instanceof Error ? detail.error.message : 'Could not load this entry.'}
          </AlertDescription>
        </Alert>
      </div>
    )
  }
  const { before, after, metadata } = detail.data.data
  return (
    <div className="flex min-w-0 flex-col gap-4 px-4 pb-4">
      <JsonBlock label="Before" value={before} />
      <JsonBlock label="After" value={after} />
      <JsonBlock label="Details" value={metadata} />
    </div>
  )
}

function JsonBlock({ label, value }: { label: string; value: unknown }) {
  const empty =
    value === null ||
    value === undefined ||
    (typeof value === 'object' && Object.keys(value as object).length === 0)
  return (
    <section className="flex max-w-full min-w-0 flex-col gap-1.5">
      <h3 className="text-muted-foreground text-xs font-medium">{label}</h3>
      {empty ? (
        <p className="text-muted-foreground text-xs">Not recorded</p>
      ) : (
        <pre
          data-testid={`audit-json-${label.toLowerCase()}`}
          className="bg-surface-muted max-w-full overflow-x-auto rounded-md p-3 font-mono text-xs"
        >
          {JSON.stringify(value, null, 2)}
        </pre>
      )}
    </section>
  )
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </>
  )
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span className="font-mono text-xs break-all">{children}</span>
}
