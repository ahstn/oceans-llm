'use client'

// Preserve the ReUI menu sections together so updates retain their shared feature gates.
/* eslint-disable max-lines-per-function, complexity */

import { memo } from 'react'
import type { HTMLAttributes, ReactNode } from 'react'
import { getColumnHeaderLabel, useDataGrid } from '@/components/reui/data-grid/data-grid'
import type { DataGridFeatures } from '@/components/reui/data-grid/data-grid'
import { Subscribe } from '@tanstack/react-table'
import type { Column } from '@tanstack/react-table'

import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { AppIcon } from '@/components/icons/app-icon'
import {
  ArrowDown02Icon,
  ArrowUp02Icon,
  UnfoldMoreIcon,
  Tick02Icon,
  ArrowLeft03Icon,
  ArrowRight03Icon,
  ArrowLeft02Icon,
  ArrowRight02Icon,
  SlidersHorizontalIcon,
  PinOffIcon,
} from '@hugeicons/core-free-icons'

interface DataGridColumnHeaderProps<
  TData extends object,
  TValue,
> extends HTMLAttributes<HTMLDivElement> {
  column: Column<DataGridFeatures, TData, TValue>
  /** When omitted, uses `column.columnDef.meta.headerTitle`, then a string `columnDef.header`, then `column.id`. */
  title?: string
  icon?: ReactNode
  /** Reserved; pin controls are gated by tableLayout.columnsPinnable + column.getCanPin(). */
  pinnable?: boolean
  filter?: ReactNode
  visibility?: boolean
}

function appendColumnMenuSection(items: ReactNode[], section: string, ...content: ReactNode[]) {
  if (items.length > 0) {
    items.push(<DropdownMenuSeparator key={`sep-${section}`} />)
  }
  items.push(...content)
}

/** Keeps section order and separators together, and builds items only when the menu opens. */
function DataGridColumnHeaderMenu<TData extends object, TValue>({
  column,
  filter,
  visibility = false,
}: Pick<DataGridColumnHeaderProps<TData, TValue>, 'column' | 'filter' | 'visibility'>) {
  const { i18n, table, props } = useDataGrid()

  // TanStack's columnOrder defaults to [] until a consumer seeds it; fall
  // back to the definition order so Move Left/Right work out of the box.
  const columnOrderState = table.state.columnOrder
  const columnOrder =
    columnOrderState.length > 0
      ? columnOrderState
      : table.getAllLeafColumns().map((leafColumn) => leafColumn.id)
  const isSorted = column.getIsSorted()
  const isPinned = column.getIsPinned()
  const canSort = column.getCanSort()
  const canPin = column.getCanPin()

  const columnIndex = columnOrder.indexOf(column.id)
  const canMoveLeft = columnIndex > 0
  const canMoveRight = columnIndex < columnOrder.length - 1

  const items: ReactNode[] = []

  // Filter section
  if (filter) {
    appendColumnMenuSection(
      items,
      'filter',
      <DropdownMenuGroup key="group-filter">
        <DropdownMenuLabel key="filter">{filter}</DropdownMenuLabel>
      </DropdownMenuGroup>,
    )
  }

  // Sort section
  if (canSort) {
    appendColumnMenuSection(
      items,
      'sort',
      <DropdownMenuItem
        key="sort-asc"
        onClick={() => {
          if (isSorted === 'asc') {
            column.clearSorting()
          } else {
            column.toggleSorting(false)
          }
        }}
        disabled={!canSort}
      >
        <AppIcon icon={ArrowUp02Icon} />
        <span className="grow">{i18n.labels.sortAscending}</span>
        {isSorted === 'asc' && <AppIcon icon={Tick02Icon} className="text-primary opacity-100!" />}
      </DropdownMenuItem>,
      <DropdownMenuItem
        key="sort-desc"
        onClick={() => {
          if (isSorted === 'desc') {
            column.clearSorting()
          } else {
            column.toggleSorting(true)
          }
        }}
        disabled={!canSort}
      >
        <AppIcon icon={ArrowDown02Icon} />
        <span className="grow">{i18n.labels.sortDescending}</span>
        {isSorted === 'desc' && <AppIcon icon={Tick02Icon} className="text-primary opacity-100!" />}
      </DropdownMenuItem>,
    )
  }

  // Pin section
  if (props.tableLayout?.columnsPinnable && canPin) {
    appendColumnMenuSection(
      items,
      'pin',
      <DropdownMenuItem
        key="pin-left"
        onClick={() => column.pin(isPinned === 'start' ? false : 'start')}
      >
        <AppIcon icon={ArrowLeft03Icon} aria-hidden={true} />
        <span className="grow">{i18n.labels.pinColumnStart}</span>
        {isPinned === 'start' && (
          <AppIcon icon={Tick02Icon} className="text-primary opacity-100!" />
        )}
      </DropdownMenuItem>,
      <DropdownMenuItem
        key="pin-right"
        onClick={() => column.pin(isPinned === 'end' ? false : 'end')}
      >
        <AppIcon icon={ArrowRight03Icon} aria-hidden={true} />
        <span className="grow">{i18n.labels.pinColumnEnd}</span>
        {isPinned === 'end' && <AppIcon icon={Tick02Icon} className="text-primary opacity-100!" />}
      </DropdownMenuItem>,
    )
  }

  // Move section
  if (props.tableLayout?.columnsMovable) {
    appendColumnMenuSection(
      items,
      'move',
      <DropdownMenuItem
        key="move-left"
        onClick={() => {
          if (columnIndex > 0) {
            const newOrder = [...columnOrder]
            const [movedColumn] = newOrder.splice(columnIndex, 1)
            newOrder.splice(columnIndex - 1, 0, movedColumn)
            table.setColumnOrder(newOrder)
          }
        }}
        disabled={!canMoveLeft || isPinned !== false}
      >
        <AppIcon icon={ArrowLeft02Icon} aria-hidden={true} />
        <span>{i18n.labels.moveColumnStart}</span>
      </DropdownMenuItem>,
      <DropdownMenuItem
        key="move-right"
        onClick={() => {
          if (columnIndex < columnOrder.length - 1) {
            const newOrder = [...columnOrder]
            const [movedColumn] = newOrder.splice(columnIndex, 1)
            newOrder.splice(columnIndex + 1, 0, movedColumn)
            table.setColumnOrder(newOrder)
          }
        }}
        disabled={!canMoveRight || isPinned !== false}
      >
        <AppIcon icon={ArrowRight02Icon} aria-hidden={true} />
        <span>{i18n.labels.moveColumnEnd}</span>
      </DropdownMenuItem>,
    )
  }

  // Visibility section
  if (props.tableLayout?.columnsVisibility && visibility) {
    appendColumnMenuSection(
      items,
      'visibility',
      <DropdownMenuSub key="visibility">
        <DropdownMenuSubTrigger>
          <AppIcon icon={SlidersHorizontalIcon} />
          <span>{i18n.labels.columnsMenu}</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuGroup>
            {table
              .getAllColumns()
              .filter((col) => col.getCanHide())
              .map((col) => (
                <DropdownMenuCheckboxItem
                  key={col.id}
                  checked={col.getIsVisible()}
                  onSelect={(event) => event.preventDefault()}
                  onCheckedChange={(value) => col.toggleVisibility(!!value)}
                  className="capitalize"
                >
                  {getColumnHeaderLabel(col)}
                </DropdownMenuCheckboxItem>
              ))}
          </DropdownMenuGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>,
    )
  }

  return <DropdownMenuGroup>{items}</DropdownMenuGroup>
}

function DataGridColumnHeaderInner<TData extends object, TValue>({
  column,
  title,
  icon,
  className,
  filter,
  visibility = false,
}: DataGridColumnHeaderProps<TData, TValue>) {
  const { i18n, isLoading, props } = useDataGrid()
  const resolvedTitle = title ?? getColumnHeaderLabel(column)

  const isSorted = column.getIsSorted()
  const isPinned = column.getIsPinned()
  const canSort = column.getCanSort()
  const canPin = column.getCanPin()
  const canResize = column.getCanResize()

  const handleSort = () => {
    if (isSorted === 'asc') {
      column.toggleSorting(true)
    } else if (isSorted === 'desc') {
      column.clearSorting()
    } else {
      column.toggleSorting(false)
    }
  }

  const headerLabelClassName = cn(
    'text-secondary-foreground/80 inline-flex h-full items-center gap-1.5 text-[0.8125rem] leading-[calc(1.125/0.8125)] font-normal [&_svg]:size-3.5 [&_svg]:opacity-60',
    className,
  )

  const headerButtonClassName = cn(
    'text-secondary-foreground/80 hover:bg-secondary data-[state=open]:bg-secondary hover:text-foreground data-[state=open]:text-foreground h-6 rounded-lg px-2 font-normal',
    className,
  )

  const sortIcon =
    canSort &&
    (isSorted === 'desc' ? (
      <AppIcon data-icon="inline-end" icon={ArrowDown02Icon} aria-hidden={true} />
    ) : isSorted === 'asc' ? (
      <AppIcon data-icon="inline-end" icon={ArrowUp02Icon} aria-hidden={true} />
    ) : (
      <AppIcon data-icon="inline-end" icon={UnfoldMoreIcon} className="mt-px" aria-hidden={true} />
    ))

  const hasPinControls = props.tableLayout?.columnsPinnable && canPin
  const hasControls =
    props.tableLayout?.columnsMovable ||
    (props.tableLayout?.columnsVisibility && visibility) ||
    hasPinControls ||
    filter

  if (hasControls) {
    return (
      <div className="-ms-2 flex h-full items-center justify-between gap-1.5">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" className={headerButtonClassName} disabled={isLoading}>
              {icon}
              {resolvedTitle}
              {sortIcon}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-40" align="start">
            <DataGridColumnHeaderMenu column={column} filter={filter} visibility={visibility} />
          </DropdownMenuContent>
        </DropdownMenu>
        {hasPinControls && isPinned && (
          <Button
            size="icon-sm"
            variant="ghost"
            className="-me-1 size-7 rounded-lg"
            onClick={() => column.pin(false)}
            aria-label={i18n.labels.unpinColumn(resolvedTitle)}
            title={i18n.labels.unpinColumn(resolvedTitle)}
          >
            <AppIcon icon={PinOffIcon} className="opacity-50!" aria-hidden={true} />
          </Button>
        )}
      </div>
    )
  }

  if (canSort || (props.tableLayout?.columnsResizable && canResize)) {
    return (
      <div className="-ms-2 flex h-full items-center">
        <Button
          variant="ghost"
          className={headerButtonClassName}
          disabled={isLoading}
          onClick={handleSort}
        >
          {icon}
          {resolvedTitle}
          {sortIcon}
        </Button>
      </div>
    )
  }

  return (
    <div className={headerLabelClassName}>
      {icon}
      {resolvedTitle}
    </div>
  )
}

const DataGridColumnHeaderMemo = memo(DataGridColumnHeaderInner) as <TData extends object, TValue>(
  props: DataGridColumnHeaderProps<TData, TValue> & {
    /** Internal: the state slices the header re-renders on. Not part of the public API. */
    subscribedState?: unknown
  },
) => ReactNode

/**
 * Sort and pin state reaches this header through builder calls on `column`
 * (`getIsSorted()`, `getIsPinned()`), and `column` is a stable reference. That
 * combination is the one v9's fresh-table-per-state-change does NOT cover:
 * React Compiler is free to memoize against the stable column and never
 * re-evaluate those reads, which shows up as frozen sort arrows and pin
 * controls. The `Subscribe` below turns the slices this header actually reads
 * into a real reactive dependency, and threading the selection through as a
 * prop is what lets it past the `memo` - which would otherwise see unchanged
 * props and skip the render anyway.
 */
function DataGridColumnHeader<TData extends object, TValue>(
  props: DataGridColumnHeaderProps<TData, TValue>,
) {
  const { table } = useDataGrid()

  return (
    <Subscribe
      source={table.store}
      selector={(state) => ({
        sorting: state.sorting,
        columnPinning: state.columnPinning,
        columnOrder: state.columnOrder,
        columnVisibility: state.columnVisibility,
      })}
    >
      {(subscribed) => <DataGridColumnHeaderMemo {...props} subscribedState={subscribed} />}
    </Subscribe>
  )
}

export { DataGridColumnHeader, type DataGridColumnHeaderProps }
