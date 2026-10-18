import { HugeiconsIcon, type IconSvgElement } from '@hugeicons/react'

type IconLike = IconSvgElement

// 2.5 keeps the small checkbox tick legible.
export type AppIconStroke = 1 | 1.2 | 1.5 | 2.5

interface AppIconProps {
  icon: IconLike
  size?: number
  stroke?: AppIconStroke
  color?: string
  className?: string
  'aria-hidden'?: boolean
  'data-icon'?: 'inline-start' | 'inline-end'
}

export function AppIcon({
  icon,
  size = 22,
  stroke = 1.5,
  color = 'currentColor',
  className,
  'aria-hidden': ariaHidden,
  'data-icon': dataIcon,
}: AppIconProps) {
  return (
    <HugeiconsIcon
      icon={icon}
      size={size}
      strokeWidth={stroke}
      color={color}
      className={className}
      aria-hidden={ariaHidden}
      data-icon={dataIcon}
    />
  )
}
