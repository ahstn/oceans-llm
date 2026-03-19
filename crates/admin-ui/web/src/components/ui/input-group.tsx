import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '@/lib/utils'

export function InputGroup({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      role="group"
      className={cn(
        'group border-border bg-muted focus-within:border-ring focus-within:ring-primary/20 relative flex min-h-11 w-full min-w-0 items-stretch overflow-hidden rounded-md border focus-within:ring-2',
        className,
      )}
      {...props}
    />
  )
}

const inputGroupAddonVariants = cva(
  'flex items-center justify-center px-3 text-sm text-muted-foreground/80',
  {
    variants: {
      align: {
        'inline-start': 'order-first border-r border-border',
        'inline-end': 'order-last border-l border-border',
        'block-start': 'order-first w-full justify-start border-b border-border',
        'block-end': 'order-last w-full justify-start border-t border-border',
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
      'text-foreground placeholder:text-muted-foreground/80 min-w-0 flex-1 border-0 bg-transparent px-3.5 py-2.5 text-sm outline-none disabled:cursor-not-allowed disabled:opacity-50',
      className,
    )}
    {...props}
  />
))
InputGroupInput.displayName = 'InputGroupInput'
