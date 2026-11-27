import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import { Add01Icon, Cancel01Icon, FilterHorizontalIcon } from '@hugeicons/core-free-icons'

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
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
} from '@/components/ui/input-group'
import { formatCount } from '@/lib/format'
import { cn } from '@/lib/utils'

import {
  dayOptions,
  groupParams,
  type FilterField,
  type FilterGroup,
  type FilterPatch,
  type PickerSource,
} from './filter-fields'
import { readyFilterOptions, type FilterOptionsState } from './filter-options'
import { FilterValuePicker } from './filter-value-picker'

/**
 * Loads the options of catalog-backed pickers. `shows` says whether a chip using a catalog is
 * visible, so a catalog loads only once its chip shows. Return `undefined` for unknown sources.
 * Pass a module-level hook so it is called the same way on every render.
 */
export type CatalogOptionsHook = (
  shows: (source: string) => boolean,
) => (source: string) => FilterOptionsState | undefined

const noCatalogOptions: CatalogOptionsHook = () => () => undefined

interface FilterToolbarProps<Param extends string> {
  groups: FilterGroup<Param>[]
  filters: FilterPatch<Param>
  shownCount: number
  totalCount: number
  isPending: boolean
  /** Prefixes the toolbar's test ids, such as `agent-session` for `agent-session-filter-chips`. */
  testIdPrefix: string
  useCatalogOptions?: CatalogOptionsHook
  /** A search box at the start of the row, such as the debounced model or user search. */
  search?: ReactNode
  /** Controls that sit beside `Filters`, such as the column toggle. */
  actions?: ReactNode
  /** Merges `patch` into the current URL search; keys set to `undefined` are removed. */
  onApply: (patch: FilterPatch<Param>) => void
}

/**
 * DESIGN.md's filter toolbar: a `Filters` menu that adds removable `[field | is | value | ×]`
 * chips. Pickers apply as soon as a value is chosen; typed chips apply on Enter or blur.
 */
// Chip drafts, pickers, and the add-filter menus share one URL-backed filter state.
// oxlint-disable-next-line eslint/max-lines-per-function
export function FilterToolbar<Param extends string>({
  groups,
  filters,
  shownCount,
  totalCount,
  isPending,
  testIdPrefix,
  useCatalogOptions = noCatalogOptions,
  search,
  actions,
  onApply,
}: FilterToolbarProps<Param>) {
  const fields = groups.flatMap((group) => group.fields)
  const params = groupParams(groups)
  const [drafts, setDrafts] = useState<FilterPatch<Param>>(filters)
  const [addedFields, setAddedFields] = useState<string[]>([])
  const [focusField, setFocusField] = useState<string | null>(null)
  const [syncedFilters, setSyncedFilters] = useState(filters)
  // The filters most recently sent to `onApply`. While its navigation is pending, `filters` still
  // holds the earlier URL, so changes are compared against what was sent instead. Once nothing is
  // pending the URL is the truth again, so a navigation that was cancelled can be retried.
  const [appliedFilters, setAppliedFilters] = useState<FilterPatch<Param>>(filters)
  const submittedFilters = isPending ? appliedFilters : filters

  // Adopt URL-backed filter changes (navigation, Back/Forward, Clear) without discarding chip
  // edits that were typed while a previous navigation was still loading.
  if (syncedFilters !== filters) {
    setSyncedFilters(filters)
    setAppliedFilters(filters)
    setDrafts((current) => rebaseDrafts(params, current, syncedFilters, filters))
  }

  const addedFieldSet = new Set(addedFields)
  const visibleFields = fields.filter(
    (field) => isFieldActive(filters, field) || addedFieldSet.has(field.key),
  )
  const activeCount = fields.filter((field) => isFieldActive(filters, field)).length
  const catalogOptions = useCatalogOptions((source) =>
    visibleFields.some((field) => field.picker?.source === source),
  )

  function pickerOptions(source: PickerSource): FilterOptionsState {
    if (Array.isArray(source)) return readyFilterOptions(source)
    if (source === 'dates') return readyFilterOptions(dayOptions(new Date()))
    return catalogOptions(source) ?? readyFilterOptions([])
  }

  function addField(key: string) {
    setAddedFields((current) => (current.includes(key) ? current : [...current, key]))
    setFocusField(key)
  }

  // A half-typed pair stays a draft; every other change, such as a pick, still applies.
  function commit(next: FilterPatch<Param> = drafts) {
    const patch = pickFilterParams(params, next)
    for (const field of fields) {
      if (field.pair && isPartialPair(next, field)) {
        for (const param of field.params) delete patch[param]
      }
    }
    const nextApplied = { ...submittedFilters, ...patch }
    if (!sameFilters(params, nextApplied, submittedFilters)) {
      setAppliedFilters(nextApplied)
      onApply(patch)
    }
  }

  // Picked values have no draft stage, so they apply as soon as they are chosen.
  function pick(param: Param, value: string) {
    const next = { ...drafts, [param]: value }
    setDrafts(next)
    commit(next)
  }

  function removeField(field: FilterField<Param>) {
    setAddedFields((current) => current.filter((key) => key !== field.key))
    const next = { ...drafts }
    for (const param of field.params) {
      next[param] = undefined
    }
    setDrafts(next)
    commit(next)
  }

  function clearAll() {
    const cleared = pickFilterParams(params, {})
    setAddedFields([])
    setDrafts((current) => ({ ...current, ...cleared }))
    setAppliedFilters({ ...submittedFilters, ...cleared })
    onApply(cleared)
  }

  const partialPair = fields.find((field) => field.pair && isPartialPair(drafts, field))

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid={`${testIdPrefix}-toolbar`}>
      <div
        className={cn(
          'flex min-w-0 flex-wrap items-center gap-3',
          search ? 'justify-between' : 'justify-end',
        )}
      >
        {search}
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-muted-foreground text-sm tabular-nums" role="status">
            {isPending
              ? 'Loading…'
              : `Showing ${formatCount(shownCount)} of ${formatCount(totalCount)}`}
          </span>
          <div className="flex items-center gap-2">
            <FilterFieldMenu groups={groups} visibleFields={visibleFields} onSelect={addField}>
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
            {actions}
          </div>
        </div>
      </div>

      {visibleFields.length > 0 ? (
        <div
          className="flex min-w-0 flex-wrap items-center gap-2"
          data-testid={`${testIdPrefix}-filter-chips`}
        >
          {visibleFields.map((field) => (
            <FilterChip
              key={field.key}
              field={field}
              drafts={drafts}
              testIdPrefix={testIdPrefix}
              autoFocus={focusField === field.key}
              pickerOptions={field.picker ? pickerOptions(field.picker.source) : undefined}
              onPick={pick}
              onDraftChange={(param, value) =>
                setDrafts((current) => ({ ...current, [param]: value }))
              }
              onCommit={() => commit()}
              onRemove={() => removeField(field)}
            />
          ))}
          <FilterFieldMenu groups={groups} visibleFields={visibleFields} onSelect={addField}>
            <Button type="button" variant="outline" size="icon-sm" aria-label="Add filter">
              <AppIcon icon={Add01Icon} size={14} stroke={1.5} aria-hidden />
            </Button>
          </FilterFieldMenu>
          <Button type="button" variant="ghost" size="sm" onClick={clearAll}>
            Clear
          </Button>
        </div>
      ) : null}

      {partialPair?.pair ? (
        <Alert>
          <AlertTitle>{partialPair.pair.incompleteTitle}</AlertTitle>
          <AlertDescription>{partialPair.pair.incompleteDescription}</AlertDescription>
        </Alert>
      ) : null}
    </div>
  )
}

function FilterFieldMenu<Param extends string>({
  groups,
  visibleFields,
  onSelect,
  children,
}: {
  groups: FilterGroup<Param>[]
  visibleFields: FilterField<Param>[]
  onSelect: (key: string) => void
  children: ReactNode
}) {
  const pickedField = useRef(false)
  const visibleKeys = new Set(visibleFields.map((field) => field.key))
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      {/* After a pick, focus moves to the new chip; Escape still returns it to the trigger. */}
      <DropdownMenuContent
        align="end"
        className="w-56"
        onCloseAutoFocus={(event) => {
          if (pickedField.current) event.preventDefault()
          pickedField.current = false
        }}
      >
        <DropdownMenuLabel>Filter by</DropdownMenuLabel>
        {groups.map((group, index) => (
          <Fragment key={group.label}>
            {index > 0 ? <DropdownMenuSeparator /> : null}
            <DropdownMenuGroup aria-label={group.label}>
              {group.fields.map((field) => (
                <DropdownMenuItem
                  key={field.key}
                  disabled={visibleKeys.has(field.key)}
                  onSelect={() => {
                    pickedField.current = true
                    onSelect(field.key)
                  }}
                >
                  <AppIcon icon={field.icon} size={16} stroke={1.5} aria-hidden />
                  {field.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuGroup>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function FilterChip<Param extends string>({
  field,
  drafts,
  testIdPrefix,
  autoFocus,
  pickerOptions,
  onPick,
  onDraftChange,
  onCommit,
  onRemove,
}: {
  field: FilterField<Param>
  drafts: FilterPatch<Param>
  testIdPrefix: string
  autoFocus: boolean
  pickerOptions?: FilterOptionsState
  onPick: (param: Param, value: string) => void
  onDraftChange: (param: Param, value: string) => void
  onCommit: () => void
  onRemove: () => void
}) {
  const firstInputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!autoFocus || field.picker) return
    // Defer past the dropdown's own focus restoration on close.
    const handle = setTimeout(() => firstInputRef.current?.focus(), 0)
    return () => clearTimeout(handle)
  }, [autoFocus, field.picker])

  const testIdFor = (param: Param) => `${testIdPrefix}-filter-${param.replaceAll('_', '-')}`

  function inputProps(param: Param, label: string, placeholder: string) {
    return {
      'aria-label': label,
      'data-testid': testIdFor(param),
      // Chips span the row on narrow screens, so their inputs share it instead of overflowing.
      className: 'min-w-0 flex-1 sm:w-36 sm:flex-none',
      placeholder,
      value: draftText(drafts[param]),
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

  function chipValue() {
    const [param, valueParam] = field.params
    if (field.pair && valueParam) {
      return (
        <>
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps(param, field.pair.keyLabel, field.pair.keyPlaceholder)}
            className="min-w-0 flex-1 sm:w-24 sm:flex-none"
          />
          <InputGroupText className="border-x px-2">is</InputGroupText>
          <InputGroupInput {...inputProps(valueParam, field.pair.valueLabel, field.placeholder)} />
        </>
      )
    }
    const picker = field.picker
    return (
      <>
        <InputGroupText className="border-r px-2">is</InputGroupText>
        {picker && pickerOptions ? (
          <FilterValuePicker
            label={field.label}
            value={pickerValue(draftText(drafts[param]), picker.fromParam)}
            placeholder={field.placeholder}
            searchPlaceholder={picker.searchPlaceholder}
            options={pickerOptions}
            autoOpen={autoFocus}
            formatValue={picker.formatValue}
            testId={testIdFor(param)}
            customValue={picker.customValue}
            onSelect={(value) => onPick(param, picker.toParam ? picker.toParam(value) : value)}
          />
        ) : (
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps(param, `${field.label} filter value`, field.placeholder)}
          />
        )}
      </>
    )
  }

  return (
    <InputGroup
      className="w-full sm:w-auto"
      data-testid={`${testIdPrefix}-filter-chip-${field.key}`}
    >
      <InputGroupAddon className="text-foreground border-r pr-2 whitespace-nowrap">
        <AppIcon icon={field.icon} size={14} stroke={1.5} aria-hidden />
        {field.label}
      </InputGroupAddon>
      {chipValue()}
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

function pickerValue(value: string, fromParam?: (value: string) => string) {
  return value && fromParam ? fromParam(value) : value
}

function isFieldActive<Param extends string>(
  filters: FilterPatch<Param>,
  field: FilterField<Param>,
) {
  return field.params.some((param) => normalizedParam(filters[param]) !== undefined)
}

function isPartialPair<Param extends string>(
  values: FilterPatch<Param>,
  field: FilterField<Param>,
) {
  const [keyParam, valueParam] = field.params
  if (!valueParam) return false
  return (
    (normalizedParam(values[keyParam]) === undefined) !==
    (normalizedParam(values[valueParam]) === undefined)
  )
}

function draftText(value: string | number | null | undefined) {
  return value == null ? '' : String(value)
}

function normalizedParam(value: string | number | null | undefined) {
  return draftText(value).trim() || undefined
}

/** The chip-owned params of `values`, with absent params set to `undefined` so they clear. */
function pickFilterParams<Param extends string>(
  params: Param[],
  values: FilterPatch<Param>,
): FilterPatch<Param> {
  return Object.fromEntries(
    params.map((param) => [param, normalizedParam(values[param])]),
  ) as FilterPatch<Param>
}

function sameFilters<Param extends string>(
  params: Param[],
  left: FilterPatch<Param>,
  right: FilterPatch<Param>,
) {
  return params.every((param) => normalizedParam(left[param]) === normalizedParam(right[param]))
}

/**
 * Moves drafts onto newly applied URL filters. Params whose draft still matches the previous URL
 * value are clean and take the new value; params edited since then keep the in-progress edit.
 */
function rebaseDrafts<Param extends string>(
  params: Param[],
  drafts: FilterPatch<Param>,
  previous: FilterPatch<Param>,
  next: FilterPatch<Param>,
): FilterPatch<Param> {
  const rebased: FilterPatch<Param> = { ...next }
  for (const param of params) {
    const isDirty = normalizedParam(drafts[param]) !== normalizedParam(previous[param])
    if (isDirty) {
      rebased[param] = drafts[param]
    }
  }
  return rebased
}
