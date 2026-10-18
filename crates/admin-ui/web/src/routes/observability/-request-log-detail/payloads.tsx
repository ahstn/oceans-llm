import { useMemo } from 'react'

import {
  CodeBlock,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from '@/components/reui/code-block/code-block'
import { Badge } from '@/components/ui/badge'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'
import type { RequestLogDetailView } from '@/types/api'

export type PayloadView = 'request' | 'response' | 'split'

// Long payloads scroll inside their block rather than stretching the sheet.
const PAYLOAD_MAX_LINES = 28
// Highlighting renders a row per line, so a capture of thousands of lines is shown in part;
// Copy still copies all of it.
const PAYLOAD_PREVIEW_LINES = 500

export function PayloadViewToggle({
  view,
  onChange,
}: {
  view: PayloadView
  onChange: (view: PayloadView) => void
}) {
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      aria-label="Payload view"
      value={view}
      onValueChange={(value) => {
        if (value) {
          onChange(value as PayloadView)
        }
      }}
    >
      <ToggleGroupItem value="request">Request</ToggleGroupItem>
      <ToggleGroupItem value="response">Response</ToggleGroupItem>
      <ToggleGroupItem value="split">Split</ToggleGroupItem>
    </ToggleGroup>
  )
}

/** Request and response JSON as highlighted code blocks; split view sits side by side from xl. */
export function PayloadPanels({
  detail,
  view,
}: {
  detail: RequestLogDetailView
  view: PayloadView
}) {
  return (
    <div
      data-testid="request-log-payloads"
      className={cn('grid min-w-0 gap-4', view === 'split' && 'xl:grid-cols-2')}
    >
      {view !== 'response' ? (
        <PayloadPanel
          title="Request payload"
          truncated={detail.log.request_payload_truncated}
          payload={detail.payload?.request_json}
        />
      ) : null}
      {view !== 'request' ? (
        <PayloadPanel
          title="Response payload"
          truncated={detail.log.response_payload_truncated}
          payload={detail.payload?.response_json}
        />
      ) : null}
    </div>
  )
}

function PayloadPanel({
  title,
  truncated,
  payload,
}: {
  title: string
  truncated: boolean
  payload: unknown
}) {
  const formatted = useMemo(() => formatPayload(payload), [payload])
  const badge = truncated ? (
    <Badge variant="warning">truncated</Badge>
  ) : (
    <Badge variant="outline">full</Badge>
  )

  if (!formatted) {
    return (
      <div className="border-border flex min-w-0 flex-col overflow-hidden rounded-lg border">
        <div className="border-border bg-muted/60 flex min-h-10 items-center gap-2 border-b px-3 py-1.5">
          <span className="text-foreground text-xs font-medium">{title}</span>
          <span className="ml-auto">{badge}</span>
        </div>
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No payload stored</EmptyTitle>
            <EmptyDescription>
              Payload capture was disabled or summary-only for this request.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    )
  }

  return (
    // Prompts are long prose strings, so lines wrap rather than scroll sideways.
    <div className="flex min-w-0 flex-col gap-1.5">
      <CodeBlock
        code={formatted.preview}
        language="json"
        showLineNumbers
        wrapLines
        maxLines={PAYLOAD_MAX_LINES}
      >
        <CodeBlockHeader>
          <CodeBlockTitle>{title}</CodeBlockTitle>
          <span className="ml-auto">{badge}</span>
          <CodeBlockCopyButton
            value={formatted.full}
            labels={{ copy: `Copy ${title.toLowerCase()}` }}
          />
        </CodeBlockHeader>
      </CodeBlock>
      {formatted.lineCount > PAYLOAD_PREVIEW_LINES ? (
        <p className="text-muted-foreground text-xs">
          Showing the first {PAYLOAD_PREVIEW_LINES.toLocaleString('en-US')} of{' '}
          {formatted.lineCount.toLocaleString('en-US')} lines. Copy copies the whole payload.
        </p>
      ) : null}
    </div>
  )
}

/** The payload as indented JSON, with the lines the preview renders. */
function formatPayload(payload: unknown) {
  if (!payload) return null
  const full = JSON.stringify(payload, null, 2)
  const lines = full.split('\n')
  return {
    full,
    preview:
      lines.length > PAYLOAD_PREVIEW_LINES
        ? lines.slice(0, PAYLOAD_PREVIEW_LINES).join('\n')
        : full,
    lineCount: lines.length,
  }
}
