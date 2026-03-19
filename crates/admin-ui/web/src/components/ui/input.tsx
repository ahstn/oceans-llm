import * as React from 'react'

import { cn } from '@/lib/utils'

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {}

export const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type = 'text', ...props }, ref) => (
    <input
      ref={ref}
      type={type}
      className={cn(
        'border-border bg-muted text-foreground placeholder:text-muted-foreground/80 focus:border-ring focus:ring-primary/20 flex h-11 w-full rounded-md border px-3.5 py-2.5 text-sm transition-colors outline-none focus:ring-2 disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      {...props}
    />
  ),
)
Input.displayName = 'Input'
