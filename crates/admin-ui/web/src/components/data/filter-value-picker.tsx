import { useEffect, useState } from 'react'
import { ArrowDown01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'

import type { FilterOptionsState } from './filter-options'

function rawValue(value: string) {
  return value
}

interface FilterValuePickerProps {
  label: string
  value: string
  placeholder: string
  searchPlaceholder: string
  options: FilterOptionsState
  autoOpen: boolean
  /** Shows the selected option's hint beside it, such as a status code's reason phrase. */
  showSelectedHint?: boolean
  /** Renders a value that is not in `options`, such as a typed date. Defaults to the raw value. */
  formatValue?: (value: string) => string
  testId: string
  /** Turns typed text into a value that is not in `options`, or `null` when it is not valid. */
  customValue: (search: string) => string | null
  onSelect: (value: string) => void
}

/** A combobox that sits inside a filter chip in place of its text input. */
export function FilterValuePicker({
  label,
  value,
  placeholder,
  searchPlaceholder,
  options,
  autoOpen,
  showSelectedHint = false,
  formatValue = rawValue,
  testId,
  customValue,
  onSelect,
}: FilterValuePickerProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')

  useEffect(() => {
    if (!autoOpen) return
    // Defer past the add-filter menu closing, which would otherwise dismiss the popover.
    const handle = setTimeout(() => setOpen(true), 0)
    return () => clearTimeout(handle)
  }, [autoOpen])

  const selected = options.options.find((option) => option.value === value)
  const custom = customValue(search)
  const showCustom = custom !== null && !options.options.some((option) => option.value === custom)

  function choose(next: string) {
    setOpen(false)
    setSearch('')
    onSelect(next)
  }

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setSearch('')
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          data-slot="input-group-control"
          data-testid={testId}
          aria-label={`${label} filter value`}
          className="flex h-full min-w-0 flex-1 items-center gap-1.5 px-2 text-left text-sm outline-none sm:w-44 sm:flex-none"
        >
          <span className={cn('min-w-0 flex-1 truncate', !value && 'text-muted-foreground')}>
            {value ? (selected?.label ?? formatValue(value)) : placeholder}
          </span>
          {showSelectedHint && selected?.hint ? (
            <span className="text-muted-foreground shrink-0 truncate">{selected.hint}</span>
          ) : null}
          <AppIcon
            icon={ArrowDown01Icon}
            size={14}
            stroke={1.5}
            className="text-muted-foreground shrink-0"
            aria-hidden
          />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 p-0">
        <Command label={`Search ${label.toLowerCase()} values`}>
          <CommandInput placeholder={searchPlaceholder} value={search} onValueChange={setSearch} />
          <CommandList>
            <CommandEmpty>
              {options.status === 'loading' || options.status === 'idle'
                ? 'Loading…'
                : options.status === 'error'
                  ? 'Could not load options. Type a value instead.'
                  : 'No matches.'}
            </CommandEmpty>
            {options.options.length > 0 ? (
              <CommandGroup heading={label}>
                {options.options.map((option) => (
                  <CommandItem
                    key={option.value}
                    // Match the raw value too, so a typed `2026-11-21` still finds a preset day.
                    value={option.value}
                    keywords={[option.label, option.hint].filter((word): word is string =>
                      Boolean(word),
                    )}
                    data-checked={option.value === value}
                    onSelect={() => choose(option.value)}
                  >
                    {/* The value outranks its hint when both do not fit. */}
                    <span className="max-w-56 shrink-0 truncate">{option.label}</span>
                    {option.hint ? (
                      <span className="text-muted-foreground min-w-0 truncate text-xs">
                        {option.hint}
                      </span>
                    ) : null}
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : null}
            {showCustom ? (
              <CommandGroup forceMount>
                <CommandItem value={search} forceMount onSelect={() => choose(custom)}>
                  <span className="min-w-0 truncate">Filter by “{formatValue(custom)}”</span>
                </CommandItem>
              </CommandGroup>
            ) : null}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
