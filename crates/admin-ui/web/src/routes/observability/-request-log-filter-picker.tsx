import { useEffect, useRef, useState } from 'react'
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
import { loadAllModels } from '@/lib/model-catalog'
import { cn } from '@/lib/utils'
import type { ModelView } from '@/types/api'

export interface FilterOption {
  value: string
  label: string
  hint?: string
}

export interface FilterOptionsState {
  status: 'idle' | 'loading' | 'ready' | 'error'
  options: FilterOption[]
}

/** Status codes the gateway and its providers commonly return; any other code can be typed. */
export const statusCodeOptions: FilterOption[] = [
  { value: '200', label: '200', hint: 'OK' },
  { value: '400', label: '400', hint: 'Bad Request' },
  { value: '401', label: '401', hint: 'Unauthorized' },
  { value: '403', label: '403', hint: 'Forbidden' },
  { value: '404', label: '404', hint: 'Not Found' },
  { value: '408', label: '408', hint: 'Request Timeout' },
  { value: '413', label: '413', hint: 'Content Too Large' },
  { value: '422', label: '422', hint: 'Unprocessable Content' },
  { value: '429', label: '429', hint: 'Too Many Requests' },
  { value: '500', label: '500', hint: 'Internal Server Error' },
  { value: '502', label: '502', hint: 'Bad Gateway' },
  { value: '503', label: '503', hint: 'Service Unavailable' },
  { value: '504', label: '504', hint: 'Gateway Timeout' },
]

export const readyStatusCodeOptions: FilterOptionsState = {
  status: 'ready',
  options: statusCodeOptions,
}

/** Accepts any three-digit HTTP status typed into the status picker. */
export function customStatusCode(search: string) {
  const code = search.trim()
  return /^[1-5]\d\d$/.test(code) ? code : null
}

/** Keys that are no longer in the catalog still match older logs, so any typed key is allowed. */
export function customModelKey(search: string) {
  return search.trim() || null
}

/**
 * Loads the gateway model catalog the first time a model filter is shown, so the request list
 * does not fetch every model page on each filter navigation.
 */
export function useModelFilterOptions(enabled: boolean): FilterOptionsState {
  const [state, setState] = useState<FilterOptionsState>({ status: 'idle', options: [] })
  const requested = useRef(false)

  useEffect(() => {
    if (!enabled || requested.current) return
    requested.current = true
    setState({ status: 'loading', options: [] })
    loadAllModels()
      .then((models) => setState({ status: 'ready', options: modelFilterOptions(models) }))
      .catch(() => setState({ status: 'error', options: [] }))
  }, [enabled])

  return state
}

function modelFilterOptions(models: ModelView[]): FilterOption[] {
  const byKey = new Map<string, FilterOption>()
  for (const model of models) {
    // `id` is the model key that requests name and logs store; `model_id` is its internal UUID.
    byKey.set(model.id, {
      value: model.id,
      label: model.id,
      hint: model.alias_of ? `alias of ${model.alias_of}` : (model.provider_label ?? undefined),
    })
  }
  return [...byKey.values()].sort((left, right) => left.label.localeCompare(right.label))
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
                  ? 'Could not load options. Type a value instead.'
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
