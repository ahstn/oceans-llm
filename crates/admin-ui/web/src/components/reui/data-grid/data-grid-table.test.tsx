import { fireEvent, render, screen, cleanup } from '@testing-library/react'
import { useTable, type ColumnDef } from '@tanstack/react-table'
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  DataGrid,
  dataGridFeatures,
  type DataGridFeatures,
} from '@/components/reui/data-grid/data-grid'
import {
  DataGridTable,
  DataGridTableBodyRow,
  DataGridTableHeader,
} from '@/components/reui/data-grid/data-grid-table'

type TestRow = { name: string; action: string }

const item: TestRow = { name: 'Example', action: 'Open' }
const data = [item]
const columns: ColumnDef<DataGridFeatures, TestRow>[] = [
  { accessorKey: 'name', header: 'Name' },
  { accessorKey: 'action', header: 'Action' },
]

function RowHarness({ onRowClick }: { onRowClick?: (row: TestRow) => void }) {
  const table = useTable({ features: dataGridFeatures, data, columns })
  return (
    <DataGrid table={table} recordCount={1} onRowClick={onRowClick}>
      <table>
        <tbody>
          <DataGridTableBodyRow row={table.getRowModel().rows[0]}>
            <td>
              <button type="button">Open</button>
            </td>
          </DataGridTableBodyRow>
        </tbody>
      </table>
    </DataGrid>
  )
}

function HeaderHarness({ standalone, pinned = true }: { standalone: boolean; pinned?: boolean }) {
  const table = useTable({
    features: dataGridFeatures,
    data,
    columns,
    initialState: { columnPinning: { start: [], end: pinned ? ['action'] : [] } },
  })
  return (
    <DataGrid table={table} recordCount={1} tableLayout={{ columnsResizable: true }}>
      {standalone ? <DataGridTableHeader /> : <DataGridTable />}
    </DataGrid>
  )
}

afterEach(cleanup)

describe('DataGrid row actions', () => {
  it('lets a focused row activate once with Enter or Space', () => {
    const onRowClick = vi.fn()
    render(<RowHarness onRowClick={onRowClick} />)
    const row = screen.getByRole('row')
    expect(row).toHaveAttribute('tabindex', '0')

    expect(fireEvent.keyDown(row, { key: 'Enter' })).toBe(false)
    expect(fireEvent.keyDown(row, { key: ' ' })).toBe(false)
    fireEvent.keyDown(row, { key: 'Enter', repeat: true })
    fireEvent.keyDown(row, { key: 'ArrowDown' })

    expect(onRowClick).toHaveBeenCalledTimes(2)
    expect(onRowClick).toHaveBeenLastCalledWith(item)
  })

  it('leaves nested keyboard controls and rows without an action alone', () => {
    const onRowClick = vi.fn()
    const { rerender } = render(<RowHarness onRowClick={onRowClick} />)
    const button = screen.getByRole('button', { name: 'Open' })
    expect(fireEvent.keyDown(button, { key: 'Enter' })).toBe(true)
    expect(fireEvent.keyDown(button, { key: ' ' })).toBe(true)
    expect(onRowClick).not.toHaveBeenCalled()

    rerender(<RowHarness />)
    const row = screen.getByRole('row')
    expect(row).not.toHaveAttribute('tabindex')
    expect(fireEvent.keyDown(row, { key: 'Enter' })).toBe(true)
  })
})

describe('DataGrid header layout', () => {
  it.each([true, false])('keeps the same header and filler order with pinned=%s', (pinned) => {
    const { container, unmount } = render(<HeaderHarness standalone pinned={pinned} />)
    const standaloneHeader = container.querySelector('thead')?.innerHTML
    const headerCells = Array.from(container.querySelectorAll('thead th'))
    expect(headerCells.map((cell) => cell.getAttribute('data-col-id'))).toEqual(
      pinned ? ['name', null, 'action'] : ['name', 'action', null],
    )
    expect(container.querySelectorAll('[data-slot="data-grid-table-resize-handle"]')).toHaveLength(
      2,
    )
    unmount()

    const fullTable = render(<HeaderHarness standalone={false} pinned={pinned} />)
    expect(fullTable.container.querySelector('thead')?.innerHTML).toBe(standaloneHeader)
  })
})
