import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import {
  Add01Icon,
  AiChipIcon,
  Cancel01Icon,
  CloudServerIcon,
  CubeIcon,
  FilterHorizontalIcon,
  HashIcon,
  Leaf01Icon,
  Package01Icon,
  Search01Icon,
  Tag01Icon,
} from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
} from '@/components/ui/input-group'
import type { RequestLogFiltersInput } from '@/types/api'

export type RequestLogFilterValues = Pick<
  RequestLogFiltersInput,
  | 'q'
  | 'request_id'
  | 'model_key'
  | 'provider_key'
  | 'service'
  | 'component'
  | 'env'
  | 'tag_key'
  | 'tag_value'
>

type FilterParam = Exclude<keyof RequestLogFilterValues, 'q'>
type FilterFieldKey = Exclude<FilterParam, 'tag_key' | 'tag_value'> | 'tag'

interface FilterFieldDefinition {
  key: FilterFieldKey
  label: string
  icon: unknown
  placeholder: string
}

const filterFields: FilterFieldDefinition[] = [
  {
    key: 'model_key',
    label: 'Model',
    icon: AiChipIcon,
    placeholder: 'gpt-4.1-mini',
  },
  {
    key: 'provider_key',
    label: 'Provider',
    icon: CloudServerIcon,
    placeholder: 'openai',
  },
  {
    key: 'service',
    label: 'Service',
    icon: Package01Icon,
    placeholder: 'checkout',
  },
  { key: 'component', label: 'Component', icon: CubeIcon, placeholder: 'api' },
  { key: 'env', label: 'Environment', icon: Leaf01Icon, placeholder: 'prod' },
  { key: 'tag', label: 'Tag', icon: Tag01Icon, placeholder: 'value' },
  {
    key: 'request_id',
    label: 'Request ID',
    icon: HashIcon,
    placeholder: 'req_…',
  },
]

const searchDebounceMs = 250

interface RequestLogToolbarProps {
  filters: RequestLogFilterValues
  shownCount: number
  totalCount: number
  isPending: boolean
  onApply: (next: RequestLogFilterValues, options?: { replace?: boolean }) => void
}

/**
 * ReUI-style request log toolbar: a debounced model/user search plus removable
 * `[field | is | value | ×]` filter chips that commit to the URL on Enter or blur.
 */
// Search, chip drafts, and the add-filter menus share one URL-backed filter state.
// oxlint-disable-next-line eslint/max-lines-per-function
export function RequestLogToolbar({
  filters,
  shownCount,
  totalCount,
  isPending,
  onApply,
}: RequestLogToolbarProps) {
  const [drafts, setDrafts] = useState<RequestLogFilterValues>(filters)
  const [addedFields, setAddedFields] = useState<FilterFieldKey[]>([])
  const [focusField, setFocusField] = useState<FilterFieldKey | null>(null)

  const [syncedFilters, setSyncedFilters] = useState(filters)

  // Reset chip drafts whenever the URL-backed filters change (navigation, Back/Forward, Clear).
  if (syncedFilters !== filters) {
    setSyncedFilters(filters)
    setDrafts(filters)
  }

  const visibleFields = filterFields.filter(
    (field) => isFieldActive(filters, field.key) || addedFields.includes(field.key),
  )
  const activeCount = filterFields.filter((field) => isFieldActive(filters, field.key)).length
  const hasPartialTagFilter = Boolean(drafts.tag_key?.trim()) !== Boolean(drafts.tag_value?.trim())

  function addField(key: FilterFieldKey) {
    setAddedFields((current) => (current.includes(key) ? current : [...current, key]))
    setFocusField(key)
  }

  function commit(next: RequestLogFilterValues = drafts) {
    const tagIsPartial = Boolean(next.tag_key?.trim()) !== Boolean(next.tag_value?.trim())
    if (tagIsPartial || sameFilters(next, filters)) {
      return
    }
    onApply({ ...next, q: filters.q })
  }

  function removeField(key: FilterFieldKey) {
    setAddedFields((current) => current.filter((field) => field !== key))
    const next = { ...drafts }
    for (const param of fieldParams(key)) {
      next[param] = undefined
    }
    setDrafts(next)
    commit(next)
  }

  function clearAll() {
    setAddedFields([])
    onApply({ q: filters.q })
  }

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid="request-log-toolbar">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-3">
        <RequestLogSearch
          value={filters.q ?? ''}
          onSearch={(q) => onApply({ ...filters, q }, { replace: true })}
        />
        <div className="flex items-center gap-3">
          <span className="text-muted-foreground text-sm tabular-nums">
            {isPending ? 'Loading…' : `Showing ${shownCount} of ${totalCount}`}
          </span>
          <FilterFieldMenu visibleFields={visibleFields} onSelect={addField}>
            <Button type="button" variant="outline" size="sm" className="gap-2">
              <AppIcon
                icon={FilterHorizontalIcon}
                size={14}
                stroke={1.5}
                data-icon="inline-start"
              />
              Filters
              {activeCount > 0 ? (
                <Badge variant="secondary" className="tabular-nums">
                  {activeCount}
                </Badge>
              ) : null}
            </Button>
          </FilterFieldMenu>
        </div>
      </div>

      {visibleFields.length > 0 ? (
        <div
          className="flex min-w-0 flex-wrap items-center gap-2"
          data-testid="request-log-filter-chips"
        >
          {visibleFields.map((field) => (
            <FilterChip
              key={field.key}
              field={field}
              drafts={drafts}
              autoFocus={focusField === field.key}
              onDraftChange={(param, value) =>
                setDrafts((current) => ({ ...current, [param]: value }))
              }
              onCommit={() => commit()}
              onRemove={() => removeField(field.key)}
            />
          ))}
          <FilterFieldMenu visibleFields={visibleFields} onSelect={addField}>
            <Button type="button" variant="outline" size="icon-sm" aria-label="Add filter">
              <AppIcon icon={Add01Icon} size={14} stroke={1.5} aria-hidden />
            </Button>
          </FilterFieldMenu>
          <Button type="button" variant="ghost" size="sm" onClick={clearAll} disabled={isPending}>
            Clear
          </Button>
        </div>
      ) : null}

      {hasPartialTagFilter ? (
        <Alert>
          <AlertTitle>Incomplete tag filter</AlertTitle>
          <AlertDescription>
            Provide both a tag key and tag value to filter bespoke request tags.
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  )
}

function RequestLogSearch({ value, onSearch }: { value: string; onSearch: (q: string) => void }) {
  const [query, setQuery] = useState(value)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    // Only adopt URL changes (Back/Forward, Clear) when no keystrokes are waiting to be sent.
    if (timer.current === null) {
      setQuery(value)
    }
  }, [value])

  useEffect(() => () => clearTimeout(timer.current ?? undefined), [])

  function updateQuery(next: string) {
    setQuery(next)
    clearTimeout(timer.current ?? undefined)
    timer.current = setTimeout(() => {
      timer.current = null
      if (next.trim() !== value) {
        onSearch(next)
      }
    }, searchDebounceMs)
  }

  return (
    <InputGroup className="w-full sm:max-w-xs">
      <InputGroupInput
        type="search"
        aria-label="Search request logs by model or user"
        placeholder="Search by model or user…"
        value={query}
        onChange={(event) => updateQuery(event.target.value)}
      />
      <InputGroupAddon>
        <AppIcon icon={Search01Icon} aria-hidden />
      </InputGroupAddon>
    </InputGroup>
  )
}

function FilterFieldMenu({
  visibleFields,
  onSelect,
  children,
}: {
  visibleFields: FilterFieldDefinition[]
  onSelect: (key: FilterFieldKey) => void
  children: ReactNode
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      {/* Keep focus on the newly added chip input instead of returning it to the trigger. */}
      <DropdownMenuContent
        align="end"
        className="w-48"
        onCloseAutoFocus={(event) => event.preventDefault()}
      >
        <DropdownMenuGroup>
          <DropdownMenuLabel>Filter by</DropdownMenuLabel>
          {filterFields.map((field) => (
            <DropdownMenuItem
              key={field.key}
              disabled={visibleFields.some((visible) => visible.key === field.key)}
              onSelect={() => onSelect(field.key)}
            >
              <AppIcon icon={field.icon} size={16} stroke={1.5} aria-hidden />
              {field.label}
            </DropdownMenuItem>
          ))}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function FilterChip({
  field,
  drafts,
  autoFocus,
  onDraftChange,
  onCommit,
  onRemove,
}: {
  field: FilterFieldDefinition
  drafts: RequestLogFilterValues
  autoFocus: boolean
  onDraftChange: (param: FilterParam, value: string) => void
  onCommit: () => void
  onRemove: () => void
}) {
  const firstInputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!autoFocus) return
    // Defer past the dropdown's own focus restoration on close.
    const handle = setTimeout(() => firstInputRef.current?.focus(), 0)
    return () => clearTimeout(handle)
  }, [autoFocus])

  function inputProps(param: FilterParam, label: string, placeholder: string) {
    return {
      'aria-label': label,
      'data-testid': `request-log-filter-${param.replace('_', '-')}`,
      className: 'w-36 flex-none',
      placeholder,
      value: drafts[param] ?? '',
      onChange: (event: ChangeEvent<HTMLInputElement>) => onDraftChange(param, event.target.value),
      onBlur: onCommit,
      onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === 'Enter') {
          event.preventDefault()
          onCommit()
        }
      },
    }
  }

  return (
    <InputGroup className="w-auto" data-testid={`request-log-filter-chip-${field.key}`}>
      <InputGroupAddon className="text-foreground border-r pr-2">
        <AppIcon icon={field.icon} size={14} stroke={1.5} aria-hidden />
        {field.label}
      </InputGroupAddon>
      {field.key === 'tag' ? (
        <>
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps('tag_key', 'Tag key', 'key')}
            className="w-24 flex-none"
          />
          <InputGroupText className="border-x px-2">is</InputGroupText>
          <InputGroupInput {...inputProps('tag_value', 'Tag value', field.placeholder)} />
        </>
      ) : (
        <>
          <InputGroupText className="border-r px-2">is</InputGroupText>
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps(field.key, `${field.label} filter value`, field.placeholder)}
          />
        </>
      )}
      <InputGroupAddon align="inline-end" className="border-l pl-1">
        <InputGroupButton
          size="icon-xs"
          aria-label={`Remove ${field.label} filter`}
          onClick={onRemove}
        >
          <AppIcon icon={Cancel01Icon} size={14} stroke={1.5} aria-hidden />
        </InputGroupButton>
      </InputGroupAddon>
    </InputGroup>
  )
}

function fieldParams(key: FilterFieldKey): FilterParam[] {
  return key === 'tag' ? ['tag_key', 'tag_value'] : [key]
}

function isFieldActive(filters: RequestLogFilterValues, key: FilterFieldKey) {
  return fieldParams(key).some((param) => Boolean(filters[param]))
}

function sameFilters(left: RequestLogFilterValues, right: RequestLogFilterValues) {
  return filterFields
    .flatMap((field) => fieldParams(field.key))
    .every((param) => (left[param]?.trim() || undefined) === (right[param] || undefined))
}
