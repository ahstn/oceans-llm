import { useEffect, useState, useTransition } from 'react'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { getMcpToolVersions, saveMcpToolReview } from '@/server/mcp-policy-data.functions'
import type { McpServerView, McpToolVersionView, McpToolView } from '@/types/api'
import { diffText, lineDiff } from './-tool-diff'

/** Tools with a version waiting for review. */
function pendingReviewTools(tools: McpToolView[]) {
  return tools.filter((tool) => tool.is_active && tool.pending_schema_version != null)
}

const sections: { label: string; read: (version: McpToolVersionView | undefined) => unknown }[] = [
  { label: 'Title', read: (version) => version?.title },
  { label: 'Description', read: (version) => version?.description },
  { label: 'Input schema', read: (version) => version?.input_schema },
  { label: 'Annotations', read: (version) => version?.annotations },
  { label: 'Output schema', read: (version) => version?.output_schema },
]

/**
 * Pending tool changes on one server, each with a diff from the served
 * version and approve or reject actions.
 */
export function ToolReviewsPanel({
  server,
  tools,
  onToolUpdated,
}: {
  server: McpServerView
  tools: McpToolView[]
  onToolUpdated: (tool: McpToolView) => void
}) {
  const pending = pendingReviewTools(tools)
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
      {pending.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No pending changes</EmptyTitle>
            <EmptyDescription>Every discovered tool is reviewed.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="divide-y border-y">
          {pending.map((tool) => (
            <ToolReview key={tool.id} tool={tool} onToolUpdated={onToolUpdated} />
          ))}
        </div>
      )}
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
  const [versions, setVersions] = useState<McpToolVersionView[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()

  useEffect(() => {
    let cancelled = false
    void getMcpToolVersions({ data: { serverId: tool.server_id, toolId: tool.id } })
      .then((response) => {
        if (!cancelled) {
          setVersions(response.data.items)
        }
      })
      .catch((loadError: unknown) => {
        if (!cancelled) {
          setError(loadError instanceof Error ? loadError.message : 'Failed to load versions')
        }
      })
    return () => {
      cancelled = true
    }
  }, [tool.server_id, tool.id, tool.pending_schema_version])

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
    <div className="flex max-w-full min-w-0 flex-col gap-3 py-4" data-testid="mcp-tool-review">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate font-medium">{tool.display_name}</div>
          <div className="text-subtle-foreground text-sm">
            {tool.approved_schema_version == null
              ? `New tool, version ${tool.pending_schema_version}`
              : `Version ${tool.approved_schema_version} → ${tool.pending_schema_version}`}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="warning">
            {tool.approval_status === 'pending_change' ? 'Served as approved version' : 'Withheld'}
          </Badge>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={isPending || !waiting}
            onClick={() => review('reject')}
          >
            Reject
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={isPending || !waiting}
            onClick={() => review('approve')}
          >
            Approve
          </Button>
        </div>
      </div>
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>Versions failed to load</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : versions == null ? (
        <Skeleton className="h-24 w-full" />
      ) : (
        <div className="flex max-w-full min-w-0 flex-col gap-3">
          {sections.map((section) => {
            const before = diffText(section.read(served))
            const after = diffText(section.read(waiting))
            if (before === after) {
              return null
            }
            return (
              <DiffBlock key={section.label} label={section.label} before={before} after={after} />
            )
          })}
        </div>
      )}
    </div>
  )
}

function DiffBlock({ label, before, after }: { label: string; before: string; after: string }) {
  const lines = lineDiff(before, after)
  return (
    <div className="max-w-full min-w-0" data-testid="mcp-tool-diff">
      <div className="text-subtle-foreground mb-1 text-xs font-medium">{label}</div>
      <div className="bg-muted max-w-full min-w-0 overflow-hidden rounded-md border">
        <pre className="max-h-72 max-w-full overflow-x-auto overflow-y-auto p-3 text-xs leading-relaxed">
          <code className="block min-w-max">
            {lines.map((line, index) => (
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
      </div>
    </div>
  )
}
