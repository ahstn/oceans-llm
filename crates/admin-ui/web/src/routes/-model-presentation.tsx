import {
  AttachmentIcon,
  CircleCheckIcon,
  CodeIcon,
  LiveStreaming03Icon,
  Target02Icon,
  ToolsIcon,
  VisionIcon,
} from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import type { ModelView } from '@/types/api'

export function ModelStatusIndicator({ status }: { status: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          aria-label={status}
          className={cn(
            'inline-flex size-2.5 shrink-0 rounded-full ring-3',
            status === 'healthy' ? 'bg-success ring-success/30' : 'bg-warning ring-warning/30',
          )}
        />
      </TooltipTrigger>
      <TooltipContent sideOffset={6}>{status}</TooltipContent>
    </Tooltip>
  )
}

export function ModelAllowlistDetail({
  compact = false,
  model,
}: {
  compact?: boolean
  model: ModelView
}) {
  if (!model.allowlist) {
    return (
      <span
        className={cn(
          'text-muted-foreground inline-flex items-center gap-1.5',
          compact && 'text-sm',
        )}
      >
        <AppIcon icon={CircleCheckIcon} size={compact ? 13 : 14} stroke={1.5} />
        Unrestricted
      </span>
    )
  }

  if (compact) {
    return <CompactModelAllowlist allowlist={model.allowlist} />
  }

  const refs = [
    { label: 'Users', values: model.allowlist.users },
    { label: 'Teams', values: model.allowlist.teams },
  ].filter((entry) => entry.values.length > 0)

  if (refs.length === 0) {
    return <span className="text-muted-foreground">No users or teams listed</span>
  }

  return (
    <div className="flex min-w-0 flex-col gap-2">
      {refs.map((entry) => (
        <div
          key={entry.label}
          role="group"
          aria-label={entry.label}
          className="flex min-w-0 flex-col gap-1"
        >
          <span className="text-muted-foreground text-xs font-medium">{entry.label}</span>
          <div className="flex min-w-0 flex-wrap gap-1">
            {entry.values.map((value) => (
              <Badge key={`${entry.label}:${value}`}>{value}</Badge>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

function CompactModelAllowlist({ allowlist }: { allowlist: NonNullable<ModelView['allowlist']> }) {
  const userCount = allowlist.users.length
  const teamCount = allowlist.teams.length

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-foreground inline-flex items-center gap-1.5 text-sm">
        <AppIcon icon={CircleCheckIcon} size={13} stroke={1.5} />
        Restricted
      </span>
      {userCount > 0 || teamCount > 0 ? (
        <span className="text-muted-foreground flex flex-wrap gap-x-2 text-xs">
          {userCount > 0 ? (
            <span>{`${userCount} ${userCount === 1 ? 'User' : 'Users'}`}</span>
          ) : null}
          {teamCount > 0 ? (
            <span>{`${teamCount} ${teamCount === 1 ? 'Team' : 'Teams'}`}</span>
          ) : null}
        </span>
      ) : null}
    </div>
  )
}

export function CapabilityBadges({ model }: { model: ModelView }) {
  const capabilities = [
    model.supports_streaming ? { label: 'Streaming', icon: LiveStreaming03Icon } : null,
    model.supports_vision ? { label: 'Vision', icon: VisionIcon } : null,
    model.supports_tool_calling ? { label: 'Tool Calling', icon: ToolsIcon } : null,
    model.supports_structured_output ? { label: 'Structured Output', icon: CodeIcon } : null,
    model.supports_attachments ? { label: 'Attachments', icon: AttachmentIcon } : null,
    model.supports_decisions ? { label: 'Decisions', icon: Target02Icon } : null,
  ].filter(
    (
      value,
    ): value is {
      label: string
      icon: typeof LiveStreaming03Icon
    } => value !== null,
  )

  if (capabilities.length === 0) {
    return <span className="text-muted-foreground">—</span>
  }

  return (
    <div className="flex min-w-0 flex-wrap gap-2 py-1">
      {capabilities.map((capability) => (
        <Badge key={capability.label} variant="outline" className="gap-1.5">
          <AppIcon icon={capability.icon} size={12} stroke={1.5} />
          {capability.label}
        </Badge>
      ))}
    </div>
  )
}
