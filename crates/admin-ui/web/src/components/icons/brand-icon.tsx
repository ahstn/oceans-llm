import type { CSSProperties, HTMLAttributes } from 'react'

import anthropicIcon from '@lobehub/icons-static-svg/icons/anthropic.svg'
import awsIcon from '@lobehub/icons-static-svg/icons/aws-color.svg'
import claudeIcon from '@lobehub/icons-static-svg/icons/claude-color.svg'
import geminiIcon from '@lobehub/icons-static-svg/icons/gemini-color.svg'
import openAiIcon from '@lobehub/icons-static-svg/icons/openai.svg'
import openRouterIcon from '@lobehub/icons-static-svg/icons/openrouter.svg'
import vertexAiIcon from '@lobehub/icons-static-svg/icons/vertexai-color.svg'

import { cn } from '@/lib/utils'

export type BrandIconKey =
  | 'anthropic'
  | 'aws'
  | 'claude'
  | 'gemini'
  | 'openai'
  | 'openrouter'
  | 'vertexai'

interface BrandIconSource {
  kind: 'image' | 'mask'
  src: string
}

const BRAND_ICON_SOURCES: Record<BrandIconKey, BrandIconSource> = {
  anthropic: { kind: 'mask', src: anthropicIcon },
  aws: { kind: 'image', src: awsIcon },
  claude: { kind: 'image', src: claudeIcon },
  gemini: { kind: 'image', src: geminiIcon },
  openai: { kind: 'mask', src: openAiIcon },
  openrouter: { kind: 'mask', src: openRouterIcon },
  vertexai: { kind: 'image', src: vertexAiIcon },
}

export interface BrandIconProps extends Omit<HTMLAttributes<HTMLSpanElement>, 'children'> {
  iconKey?: string | null
  size?: number
  title?: string
}

export function BrandIcon({
  className,
  iconKey,
  size = 16,
  style,
  title,
  ...props
}: BrandIconProps) {
  const source = iconKey ? BRAND_ICON_SOURCES[iconKey as BrandIconKey] : undefined

  if (!source) {
    return (
      <span
        aria-hidden="true"
        className={cn(
          'inline-flex shrink-0 rounded-full border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)]',
          className,
        )}
        style={{ height: size, width: size, ...style }}
        {...props}
      />
    )
  }

  if (source.kind === 'mask') {
    const maskStyle: CSSProperties = {
      WebkitMask: `url(${source.src}) center / contain no-repeat`,
      mask: `url(${source.src}) center / contain no-repeat`,
      backgroundColor: 'currentColor',
      height: size,
      width: size,
      ...style,
    }

    return (
      <span
        aria-hidden={title ? undefined : 'true'}
        title={title}
        className={cn('inline-flex shrink-0 text-[var(--color-text)]', className)}
        style={maskStyle}
        {...props}
      />
    )
  }

  return (
    <span
      aria-hidden={title ? undefined : 'true'}
      className={cn('inline-flex shrink-0', className)}
      style={{ height: size, width: size, ...style }}
      title={title}
      {...props}
    >
      <img alt="" className="h-full w-full object-contain" src={source.src} />
    </span>
  )
}
