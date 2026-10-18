import type { ComponentProps, ReactNode } from 'react'

import { TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

/**
 * A column label, or a label with layout options: `align: 'right'` for numeric columns, and
 * `narrow` for a rank or index column. A label doubles as the React key, so give `key` to a
 * column whose label repeats. Headers that hold controls use `AdminTableHead` children.
 */
export type AdminTableColumn =
  | string
  | { label: string; key?: string; align?: 'right'; narrow?: boolean }

type AdminTableHeaderProps = Omit<ComponentProps<typeof TableHeader>, 'children'> &
  (
    | {
        /** Column labels in order. `false` and `null` entries are skipped, for optional columns. */
        columns: ReadonlyArray<AdminTableColumn | false | null>
        children?: never
      }
    | {
        /** `AdminTableHead` cells, for headers that hold controls or sticky columns. */
        children: ReactNode
        columns?: never
      }
  )

/** The header cell of an admin data table: muted, semibold, sentence-case label. */
export function AdminTableHead({ className, ...props }: ComponentProps<typeof TableHead>) {
  return (
    <TableHead
      className={cn('text-muted-foreground px-3 py-2 font-semibold', className)}
      {...props}
    />
  )
}

/** The header row of an admin data table (the Teams/API Keys style), on the muted surface. */
export function AdminTableHeader({
  columns,
  children,
  className,
  ...props
}: AdminTableHeaderProps) {
  return (
    <TableHeader className={cn('bg-surface-muted', className)} {...props}>
      <TableRow>
        {columns
          ? columns.map((column) => {
              if (!column) {
                return null
              }
              if (typeof column === 'string') {
                return <AdminTableHead key={column}>{column}</AdminTableHead>
              }
              return (
                <AdminTableHead
                  key={column.key ?? column.label}
                  className={cn(column.align === 'right' && 'text-right', column.narrow && 'w-16')}
                >
                  {column.label}
                </AdminTableHead>
              )
            })
          : children}
      </TableRow>
    </TableHeader>
  )
}
