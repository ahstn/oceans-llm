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

import type { FilterOptionsState } from './-request-log-filter-options'

interface FilterValuePickerProps {
  label: string
  value: string
  placeholder: string
  searchPlaceholder: string
  options: FilterOptionsState
  autoOpen: boolean
  /** Shows the selected option's hint beside it, such as a status code's reason phrase. */
  showSelectedHint?: boolean
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
        if (next) options.load?.()
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          data-slot="input-group-control"
          data-testid={testId}
          aria-label={`${label} filter value`}
          className="flex h-full w-44 min-w-0 items-center gap-1.5 px-2 text-left text-sm outline-none"
        >
          <span className={cn('min-w-0 flex-1 truncate', !value && 'text-muted-foreground')}>
            {value ? (selected?.label ?? value) : placeholder}
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
                  ? 'Could not load options. Reopen to try again, or type a value.'
                  : 'No matches.'}
            </CommandEmpty>
            {options.options.length > 0 ? (
              <CommandGroup heading={label}>
                {options.options.map((option) => (
                  <CommandItem
                    key={option.value}
                    value={[option.label, option.hint].filter(Boolean).join(' ')}
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
                  <span className="min-w-0 truncate">Filter by “{custom}”</span>
                </CommandItem>
              </CommandGroup>
            ) : null}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
