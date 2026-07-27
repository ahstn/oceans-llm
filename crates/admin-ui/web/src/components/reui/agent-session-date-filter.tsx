import { useMemo, useState } from 'react'
import { Calendar04Icon } from '@hugeicons/core-free-icons'
import { endOfDay, startOfDay } from 'date-fns'

import {
  DateSelector,
  formatDateValue,
  type DateSelectorValue,
} from '@/components/reui/date-selector'
import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Separator } from '@/components/ui/separator'

type AgentSessionDateFilterProps = {
  startedAfter?: string | null
  startedBefore?: string | null
  onChange: (range: { startedAfter?: string; startedBefore?: string }) => void
}

export function AgentSessionDateFilter({
  startedAfter,
  startedBefore,
  onChange,
}: AgentSessionDateFilterProps) {
  const [open, setOpen] = useState(false)
  const value = useMemo(
    () => dateSelectorValue(startedAfter, startedBefore),
    [startedAfter, startedBefore],
  )
  const [draft, setDraft] = useState<DateSelectorValue | undefined>(value)
  const displayText = value ? formatDateValue(value, undefined, 'MMM d, yyyy') : 'All start dates'

  function handleOpenChange(nextOpen: boolean) {
    if (nextOpen) setDraft(value)
    setOpen(nextOpen)
  }

  function apply() {
    if (!draft?.startDate) return
    onChange(searchRange(draft))
    setOpen(false)
  }

  function clear() {
    setDraft(undefined)
    onChange({})
    setOpen(false)
  }

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="w-full justify-start sm:w-auto sm:min-w-64">
          <AppIcon icon={Calendar04Icon} stroke={1.5} aria-hidden data-icon="inline-start" />
          <span className="truncate">{displayText}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        sideOffset={4}
        className="max-h-[var(--radix-popover-content-available-height)] w-auto max-w-[calc(100vw-2rem)] gap-3 overflow-hidden p-0"
      >
        <div className="min-h-0 overflow-y-auto overscroll-contain p-3">
          <DateSelector
            value={value}
            onChange={setDraft}
            allowRange
            showTwoMonths={false}
            periodTypes={['day']}
            defaultPeriodType="day"
            defaultFilterType="between"
            label="Session start"
            inputHint="Try: today, last week, or 07/20/2026"
            dayDateFormat="MMM d, yyyy"
            minYear={2020}
            maxYear={new Date().getFullYear() + 5}
          />
        </div>
        <Separator />
        <div className="flex shrink-0 items-center justify-between gap-2 p-3 pt-0">
          <Button variant="ghost" size="sm" onClick={clear} disabled={!value && !draft}>
            Clear
          </Button>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => handleOpenChange(false)}>
              Cancel
            </Button>
            <Button size="sm" onClick={apply} disabled={!draft?.startDate}>
              Apply
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function dateSelectorValue(
  startedAfter?: string | null,
  startedBefore?: string | null,
): DateSelectorValue | undefined {
  const startDate = validDate(startedAfter)
  const endDate = validDate(startedBefore)
  if (!startDate && !endDate) return undefined

  return {
    period: 'day',
    operator: startDate && endDate ? 'between' : startDate ? 'after' : 'before',
    startDate: startDate ?? endDate,
    endDate: startDate && endDate ? endDate : undefined,
  }
}

function searchRange(value: DateSelectorValue) {
  const endDate = value.endDate ?? value.startDate
  switch (value.operator) {
    case 'after':
      return { startedAfter: startOfDay(value.startDate!).toISOString() }
    case 'before':
      return { startedBefore: endOfDay(value.startDate!).toISOString() }
    case 'between':
      return {
        startedAfter: startOfDay(value.startDate!).toISOString(),
        startedBefore: endDate ? endOfDay(endDate).toISOString() : undefined,
      }
    case 'is':
      return {
        startedAfter: startOfDay(value.startDate!).toISOString(),
        startedBefore: endOfDay(value.startDate!).toISOString(),
      }
  }
}

function validDate(value?: string | null) {
  if (!value) return undefined
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? undefined : date
}
