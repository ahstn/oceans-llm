import type { CSSProperties, HTMLAttributes, ReactNode } from 'react'

import claudeCodeIcon from '@lobehub/icons-static-svg/icons/claudecode.svg?url'
import codexIcon from '@lobehub/icons-static-svg/icons/codex.svg?url'
import mastraIcon from '@lobehub/icons-static-svg/icons/mastra.svg?url'
import openCodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?url'
import piIcon from '@lobehub/icons-static-svg/icons/pi.svg?url'

import { cn } from '@/lib/utils'

const AGENT_HARNESS_ICONS: Record<string, string> = {
  claudecode: claudeCodeIcon,
  codex: codexIcon,
  mastra: mastraIcon,
  opencode: openCodeIcon,
  pi: piIcon,
}

interface AgentHarnessIconProps extends Omit<HTMLAttributes<HTMLSpanElement>, 'children'> {
  harnessKey: string
  size?: number
}

export function AgentHarnessIcon({
  className,
  harnessKey,
  size = 16,
  style,
  ...props
}: AgentHarnessIconProps) {
  const normalizedKey = harnessKey.toLowerCase().replace(/[^a-z0-9]+/g, '')
  // OMP mark from omp.sh. Keep the solid white fill instead of the source gradient.
  if (normalizedKey === 'ohmypi') {
    return (
      <span
        aria-hidden="true"
        className={cn('inline-flex shrink-0', className)}
        data-agent-harness-icon={normalizedKey}
        style={{ height: size, width: size, ...style }}
        {...props}
      >
        <svg viewBox="0 0 64 64" width="100%" height="100%">
          <path fill="#fff" d="M10 14h44v9H43v33h-9V23h-9v22h-9V23H10z" />
        </svg>
      </span>
    )
  }

  const source = AGENT_HARNESS_ICONS[normalizedKey]
  if (!source) {
    return null
  }

  const maskStyle: CSSProperties = {
    WebkitMaskImage: `url("${source}")`,
    WebkitMaskPosition: 'center',
    WebkitMaskRepeat: 'no-repeat',
    WebkitMaskSize: 'contain',
    maskImage: `url("${source}")`,
    maskPosition: 'center',
    maskRepeat: 'no-repeat',
    maskSize: 'contain',
    backgroundColor: 'currentColor',
    height: size,
    width: size,
    ...style,
  }

  return (
    <span
      aria-hidden="true"
      className={cn('inline-flex shrink-0', className)}
      data-agent-harness-icon={normalizedKey}
      style={maskStyle}
      {...props}
    />
  )
}

export function AgentHarnessLabel({
  children,
  className,
  harnessKey,
  iconSize = 16,
}: {
  children: ReactNode
  className?: string
  harnessKey: string
  iconSize?: number
}) {
  return (
    <span className={cn('inline-flex items-center gap-1.5', className)}>
      <AgentHarnessIcon harnessKey={harnessKey} size={iconSize} />
      <span>{children}</span>
    </span>
  )
}
