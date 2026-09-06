import { Loading03Icon } from '@hugeicons/core-free-icons'
import type { ComponentProps } from 'react'
import { AppIcon } from '@/components/icons/app-icon'
import { cn } from '@/lib/utils'

function Spinner({ className, ...props }: ComponentProps<'span'>) {
  return (
    <span
      data-slot="spinner"
      role="status"
      aria-label="Loading"
      className={cn('inline-flex size-4 shrink-0', className)}
      {...props}
    >
      <AppIcon icon={Loading03Icon} className="size-full animate-spin" aria-hidden />
    </span>
  )
}

export { Spinner }
