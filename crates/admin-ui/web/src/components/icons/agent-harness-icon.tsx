import type { CSSProperties, HTMLAttributes, ReactNode } from 'react'

import claudeCodeIcon from '@lobehub/icons-static-svg/icons/claudecode.svg?url'
import codexIcon from '@lobehub/icons-static-svg/icons/codex.svg?url'
import openCodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?url'
import piIcon from '@lobehub/icons-static-svg/icons/pi.svg?url'

import { cn } from '@/lib/utils'

const AGENT_HARNESS_ICONS: Record<string, string> = {
  claudecode: claudeCodeIcon,
  codex: codexIcon,
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
  const source = AGENT_HARNESS_ICONS[normalizeHarnessKey(harnessKey)]

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
      data-agent-harness-icon={normalizeHarnessKey(harnessKey)}
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

function normalizeHarnessKey(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, '')
}
