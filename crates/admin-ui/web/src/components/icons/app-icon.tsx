import { HugeiconsIcon } from '@hugeicons/react'

type IconLike = unknown

export type AppIconStroke = 1 | 1.2 | 1.5

interface AppIconProps {
  icon: IconLike
  size?: number
  stroke?: AppIconStroke
  color?: string
  className?: string
}

export function AppIcon({
  icon,
  size = 16,
  stroke = 1.2,
  color = 'currentColor',
  className,
}: AppIconProps) {
  return (
    <HugeiconsIcon
      icon={icon}
      size={size}
      strokeWidth={stroke}
      color={color}
      className={className}
    />
  )
}
