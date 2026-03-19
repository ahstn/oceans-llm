import * as React from 'react'

import { cn } from '@/lib/utils'

export const Alert = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      role="alert"
      className={cn(
        'border-border bg-muted text-muted-foreground rounded-md border px-4 py-3 text-sm',
        className,
      )}
      {...props}
    />
  ),
)
Alert.displayName = 'Alert'

export const AlertTitle = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p ref={ref} className={cn('text-foreground font-semibold', className)} {...props} />
))
AlertTitle.displayName = 'AlertTitle'

export const AlertDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p ref={ref} className={cn('text-muted-foreground mt-1 text-sm', className)} {...props} />
))
AlertDescription.displayName = 'AlertDescription'
