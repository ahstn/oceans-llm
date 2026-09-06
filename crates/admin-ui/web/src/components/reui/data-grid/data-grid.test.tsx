import { memo, useLayoutEffect } from 'react'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useTable, type ColumnDef } from '@tanstack/react-table'
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  DataGrid,
  dataGridFeatures,
  useDataGrid,
  type DataGridFeatures,
  type DataGridLayoutProps,
} from '@/components/reui/data-grid/data-grid'

type TestRow = { name: string }

const row: TestRow = { name: 'Example' }
const data = [row]
const columns: ColumnDef<DataGridFeatures, TestRow>[] = [
  { accessorKey: 'name', size: 100, meta: { autoSize: true } },
]
const fixedColumns: ColumnDef<DataGridFeatures, TestRow>[] = [{ accessorKey: 'name', size: 100 }]

const GridContent = memo(function GridContent() {
  const { props, i18n, table } = useDataGrid<TestRow>()
  return (
    <>
      <span>{props.emptyMessage ?? i18n.labels.empty}</span>
      <span>{props.tableLayout?.dense ? 'Dense rows' : 'Regular rows'}</span>
      <button type="button" onClick={() => props.onRowClick?.(row)}>
        Select row
      </button>
      <button
        type="button"
        onClick={() => props.onCellsChange?.({ source: 'clear', changes: [], rejected: [] })}
      >
        Clear cells
      </button>
      <span>Width: {table.state.columnSizing.name ?? 100}</span>
    </>
  )
})

function GridHarness(props: DataGridLayoutProps<TestRow>) {
  const table = useTable({ features: dataGridFeatures, data, columns })
  return (
    <DataGrid table={table} {...props}>
      <GridContent />
    </DataGrid>
  )
}

function AutoSizeContent({ containerWidth }: { containerWidth: number }) {
  const { autoSize, table } = useDataGrid<TestRow>()
  useLayoutEffect(() => {
    autoSize?.apply(containerWidth - table.getTotalSize())
  }, [autoSize, table, containerWidth])

  return (
    <>
      <span>Width: {table.state.columnSizing.name ?? 100}</span>
      <button type="button" onClick={() => table.setColumnSizing({ name: 180 })}>
        Resize column
      </button>
    </>
  )
}

function AutoSizeHarness({
  containerWidth,
  secondTable = false,
  autoSizeEnabled = true,
}: {
  containerWidth: number
  secondTable?: boolean
  autoSizeEnabled?: boolean
}) {
  const first = useTable({
    features: dataGridFeatures,
    data,
    columns: autoSizeEnabled ? columns : fixedColumns,
  })
  const second = useTable({ features: dataGridFeatures, data, columns })
  return (
    <DataGrid table={secondTable ? second : first} recordCount={1}>
      <AutoSizeContent containerWidth={containerWidth} />
    </DataGrid>
  )
}

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('DataGrid context updates', () => {
  it('publishes current labels and layout to memoized consumers in the same update', () => {
    const { rerender } = render(<GridHarness recordCount={1} />)
    expect(screen.getByText('No data available')).toBeInTheDocument()

    rerender(
      <GridHarness
        recordCount={1}
        i18n={{ labels: { empty: 'No matching rows' } }}
        tableLayout={{ dense: true }}
      />,
    )
    expect(screen.getByText('No matching rows')).toBeInTheDocument()
    expect(screen.getByText('Dense rows')).toBeInTheDocument()

    rerender(<GridHarness recordCount={1} emptyMessage="Try another filter" />)
    expect(screen.getByText('Try another filter')).toBeInTheDocument()
    expect(screen.getByText('Regular rows')).toBeInTheDocument()
  })

  it('uses replacement row and cell-change callbacks without a table-state change', () => {
    const firstRowClick = vi.fn()
    const firstCellsChange = vi.fn()
    const nextRowClick = vi.fn()
    const nextCellsChange = vi.fn()
    const { rerender } = render(
      <GridHarness recordCount={1} onRowClick={firstRowClick} onCellsChange={firstCellsChange} />,
    )

    rerender(
      <GridHarness recordCount={1} onRowClick={nextRowClick} onCellsChange={nextCellsChange} />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Select row' }))
    fireEvent.click(screen.getByRole('button', { name: 'Clear cells' }))

    expect(nextRowClick).toHaveBeenCalledWith(row)
    expect(nextCellsChange).toHaveBeenCalledWith({ source: 'clear', changes: [], rejected: [] })
    expect(firstRowClick).not.toHaveBeenCalled()
    expect(firstCellsChange).not.toHaveBeenCalled()
  })

  it('keeps autosize bookkeeping across renders and preserves a manually resized width', async () => {
    vi.useFakeTimers()
    const { rerender } = render(<AutoSizeHarness containerWidth={250} />)
    expect(screen.getByText('Width: 250')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Resize column' }))
    rerender(<AutoSizeHarness containerWidth={300} />)
    await act(() => vi.advanceTimersByTime(150))
    expect(screen.getByText('Width: 180')).toBeInTheDocument()
  })

  it('measures the replacement table during commit without applying the old pending resize', async () => {
    vi.useFakeTimers()
    const { rerender } = render(<AutoSizeHarness containerWidth={250} />)
    expect(screen.getByText('Width: 250')).toBeInTheDocument()
    rerender(<AutoSizeHarness containerWidth={300} />)

    rerender(<AutoSizeHarness containerWidth={320} secondTable />)
    await act(() => vi.advanceTimersByTime(150))
    expect(screen.getByText('Width: 320')).toBeInTheDocument()
  })

  it('does not apply a pending resize after the column stops using autosize', async () => {
    vi.useFakeTimers()
    const { rerender } = render(<AutoSizeHarness containerWidth={250} />)
    rerender(<AutoSizeHarness containerWidth={300} />)
    expect(screen.getByText('Width: 250')).toBeInTheDocument()

    rerender(<AutoSizeHarness containerWidth={300} autoSizeEnabled={false} />)
    await act(() => vi.advanceTimersByTime(150))
    expect(screen.getByText('Width: 250')).toBeInTheDocument()
  })
})
