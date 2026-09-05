import { useState } from 'react'
import {
  ArrowDown01Icon,
  Cancel01Icon,
  Layers01Icon,
  Search01Icon,
} from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { IconTile } from '@/components/reui/icon-tile'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from '@/components/ui/field'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Skeleton } from '@/components/ui/skeleton'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { McpToolsetView, McpToolView } from '@/types/api'
import { ServerMark } from '../mcp-designs/shared'
import { selectedTools, toolGroups, type ToolsetCandidateProps, type ToolsetFilter } from './model'

export function ToolsetMark() {
  return (
    <IconTile size="sm">
      <AppIcon icon={Layers01Icon} aria-hidden />
    </IconTile>
  )
}

export function ToolsetStatus({ set }: { set: McpToolsetView }) {
  return (
    <Badge variant={set.status === 'active' ? 'outline' : 'secondary'}>
      {set.status === 'active' ? 'Active' : 'Disabled'}
    </Badge>
  )
}

export function ToolsetToolbar(props: ToolsetCandidateProps) {
  return (
    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <InputGroup className="w-full sm:max-w-xs">
        <InputGroupAddon>
          <AppIcon icon={Search01Icon} aria-hidden />
        </InputGroupAddon>
        <InputGroupInput
          aria-label="Search tool sets"
          placeholder="Search tool sets…"
          value={props.query}
          onChange={(event) => props.onQueryChange(event.target.value)}
        />
      </InputGroup>
      <ToggleGroup
        type="single"
        value={props.filter}
        spacing={1}
        variant="outline"
        onValueChange={(value) => {
          if (value) props.onFilterChange(value as ToolsetFilter)
        }}
        aria-label="Tool set status"
      >
        <ToggleGroupItem value="all">All sets</ToggleGroupItem>
        <ToggleGroupItem value="active">Active</ToggleGroupItem>
        <ToggleGroupItem value="disabled">Disabled</ToggleGroupItem>
      </ToggleGroup>
    </div>
  )
}

export function NoToolsets({ onCreate }: { onCreate: () => void }) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>No tool sets to show</EmptyTitle>
        <EmptyDescription>Try another search, or create a set for a new workflow.</EmptyDescription>
      </EmptyHeader>
      <Button variant="outline" onClick={onCreate}>
        New tool set
      </Button>
    </Empty>
  )
}

export function MembershipNotice() {
  return (
    <Alert>
      <AlertTitle>Your complete selection</AlertTitle>
      <AlertDescription>
        Saved tools are selected when you open a set. Add or remove tools, then save the full
        selection. Drafts stay with each set while you switch between them.
      </AlertDescription>
    </Alert>
  )
}

export function SelectedSetHeader(props: ToolsetCandidateProps) {
  if (!props.selected) return null
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="flex min-w-0 items-start gap-3">
        <ToolsetMark />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-lg font-semibold wrap-anywhere">{props.selected.display_name}</h2>
            <ToolsetStatus set={props.selected} />
          </div>
          <p className="text-muted-foreground font-mono text-xs wrap-anywhere">
            {props.selected.toolset_key}
          </p>
        </div>
      </div>
      <p className="text-muted-foreground text-sm wrap-anywhere">
        {props.selected.description || 'No description added.'}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={props.onEditMetadata}>
          Edit details
        </Button>
        <Button variant="ghost" size="sm" onClick={props.onAccess}>
          Manage access
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={props.onDisable}
          disabled={props.selected.status !== 'active'}
        >
          Disable set
        </Button>
      </div>
    </div>
  )
}

export function ToolCatalog(props: ToolsetCandidateProps) {
  const [query, setQuery] = useState('')
  const term = query.trim().toLowerCase()
  const groups = toolGroups
    .map((group) => ({
      ...group,
      tools: group.tools.filter((tool) =>
        `${tool.display_name} ${tool.upstream_name} ${tool.description} ${group.server.display_name}`
          .toLowerCase()
          .includes(term),
      ),
    }))
    .filter((group) => group.tools.length > 0)
  return (
    <div className="flex min-w-0 flex-col gap-4" aria-label="Tool catalog">
      <div className="flex flex-col gap-1">
        <h3 className="font-medium">Available tools</h3>
        <p className="text-muted-foreground text-sm">Choose tools across your connected servers.</p>
      </div>
      <InputGroup>
        <InputGroupAddon>
          <AppIcon icon={Search01Icon} aria-hidden />
        </InputGroupAddon>
        <InputGroupInput
          aria-label="Search available tools"
          placeholder="Search tools or servers…"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          disabled={props.catalogState !== 'ready'}
        />
      </InputGroup>
      {props.catalogState === 'loading' ? (
        <div className="flex flex-col gap-3" role="status">
          <p className="text-muted-foreground text-sm">Loading tool catalog…</p>
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-24 w-full" />
        </div>
      ) : props.catalogState === 'error' ? (
        <Alert variant="destructive">
          <AlertTitle>Tool catalog unavailable</AlertTitle>
          <AlertDescription>
            Your draft is kept. Load the catalog before saving.
            <Button variant="outline" size="sm" onClick={props.onRetryCatalog}>
              Retry catalog
            </Button>
          </AlertDescription>
        </Alert>
      ) : groups.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No matching tools</EmptyTitle>
            <EmptyDescription>Search by a tool name or server.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="flex min-w-0 flex-col gap-4">
          {groups.map(({ server, tools }) => (
            <FieldSet key={server.id} className="min-w-0 gap-2">
              <FieldLegend variant="label">
                <span className="flex items-center gap-2">
                  <ServerMark server={server} />
                  <span>{server.display_name}</span>
                  <span className="text-muted-foreground text-xs">
                    {tools.filter((tool) => tool.is_active).length} available
                  </span>
                </span>
              </FieldLegend>
              <FieldGroup className="gap-0 overflow-hidden rounded-lg border">
                {tools.map((tool) => (
                  <ToolRow
                    key={tool.id}
                    tool={tool}
                    checked={props.draftIds.includes(tool.id)}
                    onToggle={props.onToggleTool}
                  />
                ))}
              </FieldGroup>
            </FieldSet>
          ))}
        </div>
      )}
    </div>
  )
}

function ToolRow({
  tool,
  checked,
  onToggle,
}: {
  tool: McpToolView
  checked: boolean
  onToggle: (id: string, checked: boolean) => void
}) {
  return (
    <Collapsible className="min-w-0 border-b last:border-b-0">
      <div className="flex items-start gap-2 p-3">
        <Field orientation="horizontal" data-disabled={!tool.is_active} className="min-w-0">
          <Checkbox
            id={`tool-${tool.id}`}
            checked={checked}
            onCheckedChange={(value) => onToggle(tool.id, value === true)}
            disabled={!tool.is_active}
          />
          <FieldContent className="min-w-0">
            <FieldLabel htmlFor={`tool-${tool.id}`} className="wrap-anywhere">
              {tool.display_name}
            </FieldLabel>
            <FieldDescription className="wrap-anywhere">{tool.description}</FieldDescription>
            {!tool.is_active ? <Badge variant="secondary">Unavailable</Badge> : null}
          </FieldContent>
        </Field>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={`Inspect ${tool.display_name}`}>
            <AppIcon icon={ArrowDown01Icon} aria-hidden />
          </Button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent>
        <div className="bg-muted/30 flex min-w-0 flex-col gap-2 border-t p-3">
          <p className="text-muted-foreground font-mono text-xs wrap-anywhere">
            {tool.upstream_name}
          </p>
          <p className="text-xs font-medium">Input schema</p>
          <pre className="max-h-60 max-w-full overflow-auto rounded-md border p-3 text-xs">
            {JSON.stringify(tool.input_schema, null, 2)}
          </pre>
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}

export function DraftSummary(props: ToolsetCandidateProps) {
  const tools = selectedTools(props.draftIds)
  const serverCount = new Set(tools.map((tool) => tool.server_id)).size
  return (
    <div className="flex min-w-0 flex-col gap-5" aria-label="Draft selection">
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col gap-1">
          <h3 className="font-medium">Your draft</h3>
          <p className="text-muted-foreground text-xs">
            For {props.selected?.display_name ?? 'a new tool set'}
          </p>
        </div>
        <Badge variant="outline">{props.draftSaved ? 'Saved in preview' : 'Unsaved'}</Badge>
      </div>
      <div className="flex items-baseline gap-2">
        <span className="text-3xl font-semibold tracking-tight tabular-nums">{tools.length}</span>
        <span className="text-muted-foreground text-sm">
          tools selected · {serverCount} servers
        </span>
      </div>
      {tools.length > 0 ? (
        <ul className="flex min-w-0 flex-col gap-1" aria-label="Selected tools">
          {tools.map((tool) => (
            <li
              key={tool.id}
              className="flex min-w-0 items-center justify-between gap-2 rounded-md border p-2"
            >
              <div className="min-w-0">
                <p className="text-sm wrap-anywhere">{tool.display_name}</p>
                <p className="text-muted-foreground text-xs">
                  {
                    toolGroups.find((group) => group.server.id === tool.server_id)?.server
                      .display_name
                  }
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Remove ${tool.display_name}`}
                onClick={() => props.onToggleTool(tool.id, false)}
                disabled={props.catalogState !== 'ready'}
              >
                <AppIcon icon={Cancel01Icon} aria-hidden />
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-muted-foreground text-sm">
          Choose tools from the catalog to build this draft.
        </p>
      )}
      <MembershipNotice />
      <div className="flex flex-col gap-2">
        <Button
          onClick={props.onReview}
          disabled={!props.selected || props.catalogState !== 'ready'}
        >
          Review selection
        </Button>
        <Button
          variant="ghost"
          disabled={tools.length === 0 || props.catalogState !== 'ready'}
          onClick={props.onClearDraft}
        >
          Clear draft
        </Button>
      </div>
      <p className="text-muted-foreground text-xs">
        Access rules are managed separately. A tool selection does not grant access.
      </p>
    </div>
  )
}
