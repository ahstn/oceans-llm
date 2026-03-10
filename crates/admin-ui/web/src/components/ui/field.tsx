import * as React from 'react'

import { cn } from '@/lib/utils'

export function FieldGroup({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('flex flex-col gap-4', className)} {...props} />
}

export function Field({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('flex flex-col gap-2', className)} {...props} />
}

export const FieldLabel = React.forwardRef<
  HTMLLabelElement,
  React.LabelHTMLAttributes<HTMLLabelElement>
>(({ className, ...props }, ref) => (
  <label
    ref={ref}
    className={cn(
      'text-xs font-semibold tracking-[0.08em] text-[var(--color-text-soft)] uppercase',
      className,
    )}
    {...props}
  />
))
FieldLabel.displayName = 'FieldLabel'

export const FieldDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p ref={ref} className={cn('text-xs text-[var(--color-text-soft)]', className)} {...props} />
))
FieldDescription.displayName = 'FieldDescription'
