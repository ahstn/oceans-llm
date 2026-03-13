import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '@/lib/utils'

export function InputGroup({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      role="group"
      className={cn(
        'group relative flex min-h-11 w-full min-w-0 items-stretch overflow-hidden rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] focus-within:border-[color:var(--color-border-strong)] focus-within:ring-2 focus-within:ring-[var(--color-primary-soft)]',
        className,
      )}
      {...props}
    />
  )
}

const inputGroupAddonVariants = cva(
  'flex items-center justify-center px-3 text-sm text-[var(--color-text-soft)]',
  {
    variants: {
      align: {
        'inline-start': 'order-first border-r border-[color:var(--color-border)]',
        'inline-end': 'order-last border-l border-[color:var(--color-border)]',
        'block-start':
          'order-first w-full justify-start border-b border-[color:var(--color-border)]',
        'block-end': 'order-last w-full justify-start border-t border-[color:var(--color-border)]',
      },
    },
    defaultVariants: {
      align: 'inline-start',
    },
  },
)

export function InputGroupAddon({
  className,
  align,
  ...props
}: React.ComponentProps<'div'> & VariantProps<typeof inputGroupAddonVariants>) {
  return <div className={cn(inputGroupAddonVariants({ align }), className)} {...props} />
}

export const InputGroupInput = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      'min-w-0 flex-1 border-0 bg-transparent px-3.5 py-2.5 text-sm text-[var(--color-text)] outline-none placeholder:text-[var(--color-text-soft)] disabled:cursor-not-allowed disabled:opacity-50',
      className,
    )}
    {...props}
  />
))
InputGroupInput.displayName = 'InputGroupInput'
