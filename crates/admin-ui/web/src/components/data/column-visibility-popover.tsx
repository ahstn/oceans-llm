import { useId } from 'react'
import { ColumnsThreeCogIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

export interface ToggleableColumn<Key extends string> {
  key: Key
  label: string
  description?: string
}

/**
 * The `Columns` button and its `Table columns` popover, which shows or hides optional columns
 * of a desktop table. Columns that identify a row stay out of `columns`, so they never hide.
 */
export function ColumnVisibilityPopover<Key extends string>({
  description,
  columns,
  visible,
  onVisibleChange,
}: {
  description: string
  columns: ReadonlyArray<ToggleableColumn<Key>>
  visible: Readonly<Record<Key, boolean>>
  onVisibleChange: (key: Key, visible: boolean) => void
}) {
  const id = useId()
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button type="button" variant="outline" size="sm" className="gap-2">
          <AppIcon icon={ColumnsThreeCogIcon} size={14} stroke={1.5} data-icon="inline-start" />
          Columns
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64 gap-3 p-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-foreground text-sm font-medium">Table columns</h2>
          <p className="text-subtle-foreground text-xs">{description}</p>
        </div>
        <div className="flex flex-col gap-1">
          {columns.map((column) => {
            const checkboxId = `${id}-${column.key}`
            return (
              <div
                key={column.key}
                className="hover:bg-muted/50 flex items-start gap-3 rounded-md px-1 py-1.5 text-sm"
              >
                <Checkbox
                  id={checkboxId}
                  className="mt-0.5"
                  checked={visible[column.key]}
                  aria-describedby={column.description ? `${checkboxId}-description` : undefined}
                  onCheckedChange={(checked) => onVisibleChange(column.key, checked === true)}
                />
                <div className="flex min-w-0 flex-col gap-0.5">
                  <label
                    htmlFor={checkboxId}
                    className="text-foreground cursor-pointer font-medium"
                  >
                    {column.label}
                  </label>
                  {column.description ? (
                    <p id={`${checkboxId}-description`} className="text-subtle-foreground text-xs">
                      {column.description}
                    </p>
                  ) : null}
                </div>
              </div>
            )
          })}
        </div>
      </PopoverContent>
    </Popover>
  )
}
