import { useEffect, useState, type ReactNode } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'

const WIDE_LAYOUT_QUERY = '(min-width: 1280px)'

export type SegmentedTabItem = {
  value: string
  label: string
  icon?: unknown
  badge?: ReactNode
}

/**
 * Single-select segmented control used for both the top-level workspace tabs and
 * the in-detail sub-tabs. Built on ToggleGroup so it carries roving-tabindex and
 * aria semantics without pulling in a separate `tabs` dependency.
 */
export function SegmentedTabs({
  value,
  onValueChange,
  items,
  size = 'default',
  ariaLabel,
  className,
}: {
  value: string
  onValueChange: (value: string) => void
  items: SegmentedTabItem[]
  size?: 'default' | 'sm'
  ariaLabel?: string
  className?: string
}) {
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      size={size}
      value={value}
      aria-label={ariaLabel}
      onValueChange={(next) => {
        if (next) {
          onValueChange(next)
        }
      }}
      className={className}
    >
      {items.map((item) => (
        <ToggleGroupItem key={item.value} value={item.value} className="gap-2">
          {item.icon ? (
            <AppIcon icon={item.icon} stroke={1.5} aria-hidden data-icon="inline-start" />
          ) : null}
          <span>{item.label}</span>
          {item.badge}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}

/**
 * Reusable master-detail layout. On wide viewports the list and detail sit
 * side-by-side; below the breakpoint the list spans full width and the detail
 * slides in as a Sheet when an item is selected.
 */
export function MasterDetailShell({
  list,
  detail,
  detailOpen,
  onDetailOpenChange,
  detailTitle,
  detailDescription,
}: {
  list: ReactNode
  detail: ReactNode
  detailOpen: boolean
  onDetailOpenChange: (open: boolean) => void
  detailTitle: string
  detailDescription?: string
}) {
  const isWide = useWideLayout()

  if (isWide) {
    return (
      <div className="grid min-w-0 gap-4 xl:grid-cols-[22rem_minmax(0,1fr)]">
        {list}
        <div className="min-w-0">{detail}</div>
      </div>
    )
  }

  return (
    <div className="min-w-0">
      {list}
      <Sheet open={detailOpen} onOpenChange={onDetailOpenChange}>
        <SheetContent side="right" className="w-full overflow-y-auto sm:max-w-xl">
          <SheetHeader>
            <SheetTitle>{detailTitle}</SheetTitle>
            {detailDescription ? <SheetDescription>{detailDescription}</SheetDescription> : null}
          </SheetHeader>
          <div className="min-w-0 px-4 pb-6">{detail}</div>
        </SheetContent>
      </Sheet>
    </div>
  )
}

/**
 * SSR-safe wide-layout detector. Defaults to wide (desktop-first) so the initial
 * server render and hydration match on the common case, then corrects on mount.
 */
function useWideLayout() {
  const [isWide, setIsWide] = useState(true)

  useEffect(() => {
    const query = window.matchMedia(WIDE_LAYOUT_QUERY)
    const update = () => setIsWide(query.matches)
    update()
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  }, [])

  return isWide
}

export function cnListButton(active: boolean) {
  return cn(
    'w-full rounded-md border px-3 py-3 text-left transition-colors',
    active
      ? 'border-[var(--color-text)] bg-[var(--color-muted)]'
      : 'border-[var(--color-border)] hover:bg-[var(--color-muted)]',
  )
}
