import { cva, type VariantProps } from 'class-variance-authority'
import type { HTMLAttributes } from 'react'

import { cn } from '@/lib/utils'

const badgeVariants = cva(
  'inline-flex items-center rounded-sm border px-2 py-1 text-[11px] font-semibold tracking-[0.04em] uppercase transition-colors',
  {
    variants: {
      variant: {
        default:
          'border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] text-[var(--color-text-muted)]',
        success: 'border-transparent bg-[var(--color-success-soft)] text-[var(--color-success)]',
        warning: 'border-transparent bg-[var(--color-warning-soft)] text-[var(--color-warning)]',
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
