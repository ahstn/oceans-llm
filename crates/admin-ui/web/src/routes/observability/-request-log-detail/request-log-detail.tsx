import { useState } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Skeleton } from '@/components/ui/skeleton'
import type { RequestLogDetailView } from '@/types/api'

import { AttemptsTable } from './attempts'
import { formatOccurredAt, metadataBoolean, operationLabel } from './format'
import { PayloadPanels, PayloadViewToggle, type PayloadView } from './payloads'
import {
  DetailSection,
  Identifier,
  KeyMetrics,
  ModelRoute,
  RequestFacts,
  StatusBadge,
} from './summary'
import { McpInvocationsLink, McpTokenOverhead, ToolCounts } from './tools'

/**
 * The request log detail sheet's contents: the request's identity in the header, the four key
 * metrics as tiles, the secondary facts in two columns, then tools, attempts and payloads in one
 * scrolling column split by hairlines.
 */
export function RequestLogDetail({
  detail,
  pending,
  error,
  canOpenInvocations,
}: {
  detail: RequestLogDetailView | null
  pending: boolean
  error: string | null
  canOpenInvocations: boolean
}) {
  if (detail && !pending && !error) {
    return <LoadedDetail detail={detail} canOpenInvocations={canOpenInvocations} />
  }

  return (
    <>
      <SheetHeader className="border-border border-b px-6 py-4 pr-14">
        <SheetTitle>Request log detail</SheetTitle>
        <SheetDescription>Request summary, provider attempts and stored payloads.</SheetDescription>
      </SheetHeader>
      <div className="flex-1 overflow-y-auto px-6 py-5">
        {error && !pending ? (
          <Alert variant="destructive">
            <AlertTitle>Request log detail failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : (
          <div className="flex flex-col gap-4">
            <Skeleton className="h-24 w-full" />
            <Skeleton className="h-40 w-full" />
            <Skeleton className="h-64 w-full" />
          </div>
        )}
      </div>
    </>
  )
}

function LoadedDetail({
  detail,
  canOpenInvocations,
}: {
  detail: RequestLogDetailView
  canOpenInvocations: boolean
}) {
  const log = detail.log
  const operation = operationLabel(log)
  const [payloadView, setPayloadView] = useState<PayloadView>('split')

  return (
    <>
      <SheetHeader className="border-border gap-2 border-b px-6 py-4 pr-14">
        <div className="text-muted-foreground flex flex-wrap items-center gap-2 text-xs">
          <StatusBadge statusCode={log.status_code} />
          {operation ? <span className="text-subtle-foreground">{operation}</span> : null}
          {metadataBoolean(log, 'stream') ? <Badge variant="outline">stream</Badge> : null}
          <span aria-hidden>·</span>
          <span className="whitespace-nowrap tabular-nums">
            {formatOccurredAt(log.occurred_at)}
          </span>
        </div>
        <SheetTitle className="flex min-w-0 items-center">
          <span className="sr-only">Request </span>
          <Identifier value={log.request_id} label="request ID" size="lg" />
        </SheetTitle>
        <SheetDescription asChild>
          <div>
            <ModelRoute log={log} />
          </div>
        </SheetDescription>
      </SheetHeader>

      <div
        data-testid="request-log-detail-body"
        className="flex min-w-0 flex-1 flex-col gap-6 overflow-x-hidden overflow-y-auto px-6 py-5"
      >
        <KeyMetrics log={log} />
        <RequestFacts detail={detail} />

        <DetailSection
          title="MCP & tools"
          description="What the request offered the model, and what it called."
          testId="request-log-tools"
          action={canOpenInvocations ? <McpInvocationsLink requestId={log.request_id} /> : null}
        >
          <ToolCounts log={log} />
          {detail.mcp_token_overhead ? (
            <McpTokenOverhead overhead={detail.mcp_token_overhead} />
          ) : null}
        </DetailSection>

        <DetailSection
          title="Provider attempts"
          description="Ordered upstream provider execution records for this request log."
          testId="request-log-attempts"
        >
          <AttemptsTable attempts={detail.attempts} />
        </DetailSection>

        <DetailSection
          title="Payloads"
          description="Sanitized request and response bodies as stored."
          action={<PayloadViewToggle view={payloadView} onChange={setPayloadView} />}
        >
          <PayloadPanels detail={detail} view={payloadView} />
        </DetailSection>
      </div>
    </>
  )
}
