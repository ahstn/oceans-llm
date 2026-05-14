import type * as React from 'react'
import BoringAvatar from 'boring-avatars'

import { cn } from '@/lib/utils'

export const OCEANS_AVATAR_COLORS = ['#B0E0E0', '#106090', '#2080B0', '#7C3AED', '#22C55E'] as const

type GeneratedAvatarKind = 'team' | 'user'

type GeneratedAvatarProps = Omit<
  React.ComponentProps<typeof BoringAvatar>,
  'colors' | 'name' | 'variant'
> & {
  kind: GeneratedAvatarKind
  name: string
  className?: string
}

function GeneratedAvatar({ kind, name, className, size = 32, ...props }: GeneratedAvatarProps) {
  const avatarName = name.trim() || (kind === 'team' ? 'Unnamed team' : 'Unnamed user')
  const variantProps = kind === 'user' ? ({ variant: 'beam' } as const) : {}

  return (
    <BoringAvatar
      data-testid={`${kind}-generated-avatar`}
      aria-label={`${kind === 'team' ? 'Team' : 'User'} avatar for ${avatarName}`}
      className={cn('shrink-0 rounded-full', className)}
      colors={[...OCEANS_AVATAR_COLORS]}
      name={avatarName}
      size={size}
      {...variantProps}
      {...props}
    />
  )
}

export { GeneratedAvatar }
