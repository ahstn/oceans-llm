import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '@/lib/utils'

export function Empty({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex w-full min-w-0 flex-1 flex-col items-center justify-center rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] px-6 py-10 text-center text-balance shadow-[var(--shadow-soft)]',
        className,
      )}
      {...props}
    />
  )
}

export function EmptyHeader({ className, ...props }: React.ComponentProps<'div'>) {
  return <div className={cn('flex max-w-md flex-col items-center gap-3', className)} {...props} />
}

const emptyMediaVariants = cva('flex shrink-0 items-center justify-center', {
  variants: {
    variant: {
      default: '',
      icon: 'size-14 rounded-full border border-[color:var(--color-border)] bg-[var(--color-primary-soft)] text-[var(--color-primary)]',
    },
  },
  defaultVariants: {
    variant: 'default',
  },
})

export function EmptyMedia({
  className,
  variant,
  ...props
}: React.ComponentProps<'div'> & VariantProps<typeof emptyMediaVariants>) {
  return <div className={cn(emptyMediaVariants({ variant }), className)} {...props} />
}

export function EmptyTitle({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      className={cn('text-lg font-semibold text-[var(--color-text)] sm:text-xl', className)}
      {...props}
    />
  )
}

export function EmptyDescription({ className, ...props }: React.ComponentProps<'p'>) {
  return (
    <p
      className={cn(
        'text-sm text-[var(--color-text-muted)] [&>a]:underline [&>a]:underline-offset-4',
        className,
      )}
      {...props}
    />
  )
}

export function EmptyContent({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      className={cn('mt-6 flex w-full max-w-md flex-col items-center gap-3', className)}
      {...props}
    />
  )
}
