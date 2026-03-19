import { cva, type VariantProps } from 'class-variance-authority'
import type { HTMLAttributes } from 'react'

import { cn } from '@/lib/utils'

const badgeVariants = cva(
  'inline-flex items-center rounded-sm border px-2 py-1 text-[11px] font-semibold tracking-[0.04em] uppercase transition-colors',
  {
    variants: {
      variant: {
        default: 'border-border bg-muted text-muted-foreground',
        success: 'border-transparent bg-success/20 text-success',
        warning: 'border-transparent bg-warning/20 text-warning',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
)

export interface BadgeProps
  extends HTMLAttributes<HTMLDivElement>, VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />
}

export { Badge }
