import { useState } from 'react'
import { HugeiconsIcon } from '@hugeicons/react'
import { Download01Icon, RefreshIcon } from '@hugeicons/core-free-icons'

import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { SpendOwnerKind } from '@/types/api'

import {
  downloadFocusDay,
  downloadFocusRange,
  formatUtcDate,
  toWindowDays,
  type WindowDays,
} from './shared'

export type ReportControlsProps = {
  windowDays: WindowDays
  ownerKind: SpendOwnerKind
  isPending: boolean
  isPlatformAdmin: boolean
  onWindowChange: (days: WindowDays) => void
  onOwnerKindChange: (kind: SpendOwnerKind) => void
  onRefresh: () => void
}

/** Window toggle, optional owner filter, and refresh. Shared by every candidate. */
export function ReportFilters({
  windowDays,
  ownerKind,
  isPending,
  isPlatformAdmin,
  onWindowChange,
  onOwnerKindChange,
  onRefresh,
}: ReportControlsProps) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        value={String(windowDays)}
        onValueChange={(value) => {
          if (value) onWindowChange(toWindowDays(value))
        }}
        disabled={isPending}
        aria-label="Report window"
      >
        <ToggleGroupItem value="7" aria-label="Last 7 days">
          7d
        </ToggleGroupItem>
        <ToggleGroupItem value="30" aria-label="Last 30 days">
          30d
        </ToggleGroupItem>
      </ToggleGroup>
      {isPlatformAdmin ? (
        <Select
          value={ownerKind}
          onValueChange={(value) => onOwnerKindChange(value as SpendOwnerKind)}
          disabled={isPending}
        >
          <SelectTrigger size="sm" className="w-[150px]" aria-label="Owner filter">
            <SelectValue placeholder="Owner" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="all">All owners</SelectItem>
              <SelectItem value="user">User owners</SelectItem>
              <SelectItem value="service_account">Service accounts</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      ) : null}
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={onRefresh}
        disabled={isPending}
        aria-label={isPending ? 'Refreshing' : 'Refresh'}
      >
        <HugeiconsIcon icon={RefreshIcon} className={isPending ? 'animate-spin' : undefined} />
      </Button>
    </div>
  )
}

/** FOCUS CSV export menu: whole window or one UTC day. */
export function ExportMenu({
  windowDays,
  ownerKind,
  isPlatformAdmin,
  variant = 'outline',
}: Pick<ReportControlsProps, 'windowDays' | 'ownerKind' | 'isPlatformAdmin'> & {
  variant?: 'outline' | 'default' | 'secondary'
}) {
  const [exportDay, setExportDay] = useState(() => formatUtcDate(new Date()))
  const currentUserOnly = !isPlatformAdmin

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant={variant} size="sm">
          <HugeiconsIcon icon={Download01Icon} data-icon="inline-start" />
          Export FOCUS CSV
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Billing export</DropdownMenuLabel>
          <DropdownMenuItem
            onSelect={() => downloadFocusRange(windowDays, ownerKind, currentUserOnly)}
          >
            Export last {windowDays} days
          </DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <div className="flex flex-col gap-2 px-2 py-1.5" onKeyDown={(e) => e.stopPropagation()}>
            <Field>
              <FieldLabel htmlFor="focus-export-day">Single UTC day</FieldLabel>
              <div className="flex items-center gap-2">
                <Input
                  id="focus-export-day"
                  type="date"
                  value={exportDay}
                  onChange={(event) => setExportDay(event.target.value)}
                />
                <Button
                  type="button"
                  size="sm"
                  onClick={() => downloadFocusDay(exportDay, ownerKind, currentUserOnly)}
                >
                  Export
                </Button>
              </div>
            </Field>
          </div>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <p className="text-muted-foreground px-2 py-1.5 text-xs">
          Rows aggregate by UTC day. Unpriced and usage-missing requests are excluded from charge
          rows and listed in response diagnostics.
        </p>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
