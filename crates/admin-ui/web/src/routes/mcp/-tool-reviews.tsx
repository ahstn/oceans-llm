import { useState, useTransition } from 'react'
import { ArrowDown01Icon, ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { getMcpToolVersions, saveMcpToolReview } from '@/server/mcp-policy-data.functions'
import type { McpServerView, McpToolVersionView, McpToolView } from '@/types/api'
import { diffText, limitDiff, lineDiff } from './-tool-diff'

/** The server's tools with a version waiting for review. */
function pendingReviewTools(server: McpServerView, tools: McpToolView[]) {
  return tools.filter(
    (tool) => tool.server_id === server.id && tool.is_active && tool.pending_schema_version != null,
  )
}

const sections: { label: string; read: (version: McpToolVersionView | undefined) => unknown }[] = [
  { label: 'Title', read: (version) => version?.title },
  { label: 'Description', read: (version) => version?.description },
  { label: 'Input schema', read: (version) => version?.input_schema },
  { label: 'Annotations', read: (version) => version?.annotations },
  { label: 'Output schema', read: (version) => version?.output_schema },
]

/**
 * Pending tool changes on one server as collapsed rows. Expanding a row loads
 * its versions and shows a diff from the served version with approve and
 * reject actions.
 */
export function ToolReviewsPanel({
  server,
  tools,
  toolsPending,
  toolsError,
  onToolUpdated,
}: {
  server: McpServerView
  tools: McpToolView[]
  toolsPending: boolean
  toolsError: string | null
  onToolUpdated: (tool: McpToolView) => void
}) {
  const pending = pendingReviewTools(server, tools)
  return (
    <div className="flex max-w-full min-w-0 flex-col gap-4" data-testid="mcp-tool-reviews">
      <div className="flex flex-col gap-1">
        <h3 className="text-foreground text-sm font-semibold">Reviews</h3>
        <p className="text-subtle-foreground text-sm">
          {server.approve_tool_changes
            ? 'New and changed tools wait here until you approve them.'
            : 'Approval is off for this server, so discovery serves changes at once. Turn on "Require approval for tool changes" in Configuration to hold them here.'}
        </p>
      </div>
      <ReviewList
        pending={pending}
        toolsPending={toolsPending}
        toolsError={toolsError}
        onToolUpdated={onToolUpdated}
      />
    </div>
  )
}

function ReviewList({
  pending,
  toolsPending,
  toolsError,
  onToolUpdated,
}: {
  pending: McpToolView[]
  toolsPending: boolean
  toolsError: string | null
  onToolUpdated: (tool: McpToolView) => void
}) {
  // Tools still loading or failed may belong to the previously open server,
  // so nothing is listed until this server's tools are in.
  if (toolsError) {
    return (
      <Alert variant="destructive">
        <AlertTitle>Tool load failed</AlertTitle>
        <AlertDescription>{toolsError}</AlertDescription>
      </Alert>
    )
  }
  if (toolsPending) {
    return <Skeleton className="h-24 w-full" />
  }
  if (pending.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No pending changes</EmptyTitle>
          <EmptyDescription>Every discovered tool is reviewed.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
  return (
    <div className="divide-y border-y">
      {pending.map((tool) => (
        // A newer pending version starts a fresh row: no cached versions or error.
        <ToolReview
          key={`${tool.id}:${tool.pending_schema_version}`}
          tool={tool}
          onToolUpdated={onToolUpdated}
        />
      ))}
    </div>
  )
}

function ToolReview({
  tool,
  onToolUpdated,
}: {
  tool: McpToolView
  onToolUpdated: (tool: McpToolView) => void
}) {
  const [open, setOpen] = useState(false)
  const [versions, setVersions] = useState<McpToolVersionView[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()

  function handleOpenChange(next: boolean) {
    setOpen(next)
    if (next && versions == null) {
      setError(null)
      void getMcpToolVersions({ data: { serverId: tool.server_id, toolId: tool.id } })
        .then((response) => {
          setError(null)
          setVersions(response.data.items)
        })
        .catch((loadError: unknown) =>
          setError(loadError instanceof Error ? loadError.message : 'Failed to load versions'),
        )
    }
  }

  const served = versions?.find(
    (version) => version.schema_version === tool.approved_schema_version,
  )
  const waiting = versions?.find(
    (version) => version.schema_version === tool.pending_schema_version,
  )

  function review(decision: 'approve' | 'reject') {
    if (tool.pending_schema_version == null) {
      return
    }
    const schemaVersion = tool.pending_schema_version
    startTransition(async () => {
      try {
        const response = await saveMcpToolReview({
          data: { serverId: tool.server_id, toolId: tool.id, schemaVersion, decision },
        })
        onToolUpdated(response.data.tool)
        toast.success(decision === 'approve' ? 'Tool change approved' : 'Tool change rejected')
      } catch (reviewError) {
        toast.error(
          reviewError instanceof Error ? reviewError.message : 'Failed to review the change',
        )
      }
    })
  }

  return (
    <Collapsible
      className="max-w-full min-w-0"
      open={open}
      onOpenChange={handleOpenChange}
      data-testid="mcp-tool-review"
    >
      <div className="flex min-w-0 items-center justify-between gap-2 py-3">
        <div className="min-w-0">
          <div className="truncate font-medium">{tool.display_name}</div>
          <div className="text-subtle-foreground text-sm">
            {tool.approved_schema_version == null
              ? `New tool, version ${tool.pending_schema_version}`
              : `Version ${tool.approved_schema_version} → ${tool.pending_schema_version}`}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Badge variant="warning">
            {tool.approval_status === 'pending_change' ? 'Served as approved version' : 'Withheld'}
          </Badge>
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={`${open ? 'Hide' : 'Review'} ${tool.display_name} change`}
            >
              <AppIcon
                icon={open ? ArrowDown01Icon : ArrowRight01Icon}
                size={14}
                stroke={1.5}
                aria-hidden
              />
            </Button>
          </CollapsibleTrigger>
        </div>
      </div>
      <CollapsibleContent className="flex max-w-full min-w-0 flex-col gap-3 pb-4">
        <ReviewBody
          versions={versions}
          error={error}
          served={served}
          waiting={waiting}
          isPending={isPending}
          onReview={review}
        />
      </CollapsibleContent>
    </Collapsible>
  )
}

function ReviewBody({
  versions,
  error,
  served,
  waiting,
  isPending,
  onReview,
}: {
  versions: McpToolVersionView[] | null
  error: string | null
  served: McpToolVersionView | undefined
  waiting: McpToolVersionView | undefined
  isPending: boolean
  onReview: (decision: 'approve' | 'reject') => void
}) {
  return (
    <>
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>Versions failed to load</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : versions == null ? (
        <Skeleton className="h-24 w-full" />
      ) : (
        <VersionDiff served={served} waiting={waiting} />
      )}
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={isPending || !waiting}
          onClick={() => onReview('reject')}
        >
          Reject
        </Button>
        <Button
          type="button"
          size="sm"
          disabled={isPending || !waiting}
          onClick={() => onReview('approve')}
        >
          Approve
        </Button>
      </div>
    </>
  )
}

function VersionDiff({
  served,
  waiting,
}: {
  served: McpToolVersionView | undefined
  waiting: McpToolVersionView | undefined
}) {
  return (
    <div className="flex max-w-full min-w-0 flex-col gap-3">
      {sections.map((section) => {
        const before = diffText(section.read(served))
        const after = diffText(section.read(waiting))
        if (before === after) {
          return null
        }
        return <DiffBlock key={section.label} label={section.label} before={before} after={after} />
      })}
    </div>
  )
}

function DiffBlock({ label, before, after }: { label: string; before: string; after: string }) {
  const { shown, hidden } = limitDiff(lineDiff(before, after))
  return (
    <div className="max-w-full min-w-0" data-testid="mcp-tool-diff">
      <div className="text-subtle-foreground mb-1 text-xs font-medium">{label}</div>
      <div className="bg-muted max-w-full min-w-0 overflow-hidden rounded-md border">
        <pre className="max-h-72 max-w-full overflow-x-auto overflow-y-auto p-3 text-xs leading-relaxed">
          <code className="block min-w-max">
            {shown.map((line, index) => (
              <span
                // Lines repeat, so the position is the only stable key.
                // oxlint-disable-next-line react/no-array-index-key
                key={index}
                data-diff={line.kind}
                className={cn(
                  'block',
                  line.kind === 'added' && 'bg-success/15 text-success',
                  line.kind === 'removed' && 'bg-destructive/15 text-destructive',
                )}
              >
                {line.kind === 'added' ? '+ ' : line.kind === 'removed' ? '- ' : '  '}
                {line.text}
              </span>
            ))}
          </code>
        </pre>
        {hidden > 0 ? (
          <div className="text-subtle-foreground border-t px-3 py-2 text-xs">
            {hidden} more line{hidden === 1 ? '' : 's'} not shown
          </div>
        ) : null}
      </div>
    </div>
  )
}
