import { useState, useTransition } from 'react'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { saveMcpToolAnnotationOverrides, saveMcpToolTags } from '@/server/mcp-policy-data.functions'
import type { McpToolAnnotationsView, McpToolView } from '@/types/api'
import {
  UPSTREAM,
  describeHint,
  hintValue,
  isDerivedTag,
  parseHint,
  parseTagInput,
  toolHints,
  type HintKey,
} from './-tool-tags'

export function EffectiveTagBadges({ tags }: { tags: string[] }) {
  if (tags.length === 0) {
    return <span className="text-subtle-foreground text-sm">None</span>
  }
  return (
    <span className="flex flex-wrap items-center gap-1" data-testid="mcp-tool-effective-tags">
      {tags.map((tag) => (
        <Badge key={tag} variant={isDerivedTag(tag) ? 'secondary' : 'outline'}>
          {tag}
        </Badge>
      ))}
    </span>
  )
}

/**
 * Admin policy for one tool: annotation overrides shown next to the upstream
 * values, admin tags, and the tags that tag rules match.
 */
export function ToolPolicyEditor({
  tool,
  onToolUpdated,
}: {
  tool: McpToolView
  onToolUpdated: (tool: McpToolView) => void
}) {
  const [isPending, startTransition] = useTransition()
  const [overrides, setOverrides] = useState<Record<HintKey, string>>(() => ({
    read_only_hint: hintValue(tool.annotation_overrides.read_only_hint),
    destructive_hint: hintValue(tool.annotation_overrides.destructive_hint),
    idempotent_hint: hintValue(tool.annotation_overrides.idempotent_hint),
    open_world_hint: hintValue(tool.annotation_overrides.open_world_hint),
  }))
  const [tagInput, setTagInput] = useState(() => tool.admin_tags.join(', '))

  function handleSaveOverrides() {
    const annotationOverrides: McpToolAnnotationsView = {
      read_only_hint: parseHint(overrides.read_only_hint),
      destructive_hint: parseHint(overrides.destructive_hint),
      idempotent_hint: parseHint(overrides.idempotent_hint),
      open_world_hint: parseHint(overrides.open_world_hint),
    }
    startTransition(async () => {
      try {
        const response = await saveMcpToolAnnotationOverrides({
          data: { serverId: tool.server_id, toolId: tool.id, annotationOverrides },
        })
        onToolUpdated(response.data.tool)
        toast.success('Annotation overrides saved')
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save overrides')
      }
    })
  }

  function handleSaveTags() {
    startTransition(async () => {
      try {
        const response = await saveMcpToolTags({
          data: { serverId: tool.server_id, toolId: tool.id, tags: parseTagInput(tagInput) },
        })
        onToolUpdated(response.data.tool)
        setTagInput(response.data.tool.admin_tags.join(', '))
        toast.success('Tool tags saved')
      } catch (error) {
        toast.error(error instanceof Error ? error.message : 'Failed to save tags')
      }
    })
  }

  return (
    <div className="mt-4 flex max-w-full min-w-0 flex-col gap-4" data-testid="mcp-tool-policy">
      <div className="min-w-0">
        <div className="text-subtle-foreground mb-2 text-xs font-medium">Access tags</div>
        <EffectiveTagBadges tags={tool.effective_tags} />
      </div>

      <div className="min-w-0">
        <div className="text-subtle-foreground mb-2 text-xs font-medium">Annotation overrides</div>
        <div className="divide-y border-y" data-testid="mcp-tool-annotation-overrides">
          {toolHints.map((hint) => (
            <div
              key={hint.key}
              className="grid min-w-0 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_10rem] items-center gap-3 py-2 text-sm"
            >
              <span className="font-medium">{hint.label}</span>
              <span className="text-subtle-foreground truncate">
                Upstream: {describeHint(tool.annotations[hint.key])}
              </span>
              <Select
                value={overrides[hint.key]}
                onValueChange={(value) =>
                  setOverrides((current) => ({ ...current, [hint.key]: value }))
                }
              >
                <SelectTrigger aria-label={`${hint.label} override`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value={UPSTREAM}>Use upstream</SelectItem>
                    <SelectItem value="true">Yes</SelectItem>
                    <SelectItem value="false">No</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
          ))}
        </div>
        <div className="mt-2 flex justify-end">
          <Button type="button" size="sm" disabled={isPending} onClick={handleSaveOverrides}>
            Save overrides
          </Button>
        </div>
      </div>

      <div className="min-w-0">
        <label
          className="text-subtle-foreground mb-2 block text-xs font-medium"
          htmlFor={`mcp-tool-tags-${tool.id}`}
        >
          Admin tags
        </label>
        <div className="flex min-w-0 gap-2">
          <Input
            id={`mcp-tool-tags-${tool.id}`}
            value={tagInput}
            placeholder="finance, pii:low"
            onChange={(event) => setTagInput(event.target.value)}
          />
          <Button type="button" size="sm" disabled={isPending} onClick={handleSaveTags}>
            Save tags
          </Button>
        </div>
      </div>
    </div>
  )
}
