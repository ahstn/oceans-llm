import { cleanup, fireEvent, render, renderHook, screen } from '@testing-library/react'
import { useTable, type ColumnDef } from '@tanstack/react-table'
import { afterEach, describe, expect, it } from 'vitest'

import {
  DataGrid,
  dataGridFeatures,
  type DataGridFeatures,
  type DataGridLayoutProps,
} from '@/components/reui/data-grid/data-grid'
import { DataGridColumnHeader } from '@/components/reui/data-grid/data-grid-column-header'

type TestRow = { name: string; email: string }

const data: TestRow[] = [{ name: 'Example', email: 'example@example.com' }]
const columns: ColumnDef<DataGridFeatures, TestRow>[] = [
  { accessorKey: 'name', header: 'Name' },
  { accessorKey: 'email', header: 'Email' },
]
const initialState = {
  columnOrder: ['name', 'email'],
  columnPinning: { start: ['name'], end: [] },
}
const menuLayout = { columnsPinnable: true, columnsMovable: true, columnsVisibility: true }

function HeaderHarness({
  tableLayout,
  visibility = true,
  canPin = true,
  canSort = true,
  filter = false,
}: {
  tableLayout?: DataGridLayoutProps<TestRow>['tableLayout']
  visibility?: boolean
  canPin?: boolean
  canSort?: boolean
  filter?: boolean
}) {
  const table = useTable({
    features: dataGridFeatures,
    data,
    columns,
    initialState: { columnOrder: ['name', 'email'] },
    enableColumnPinning: canPin,
    enableSorting: canSort,
  })
  return (
    <DataGrid table={table} recordCount={1} tableLayout={tableLayout}>
      <DataGridColumnHeader
        column={table.getColumn('name')!}
        visibility={visibility}
        filter={filter ? <span>Filter options</span> : undefined}
      />
      <output aria-label="Sorting">{JSON.stringify(table.state.sorting)}</output>
      <output aria-label="Pinned columns">{JSON.stringify(table.state.columnPinning)}</output>
      <output aria-label="Column order">{table.state.columnOrder.join(',')}</output>
      <output aria-label="Visible columns">
        {table
          .getVisibleLeafColumns()
          .map((column) => column.id)
          .join(',')}
      </output>
    </DataGrid>
  )
}

async function openMenu() {
  fireEvent.pointerDown(screen.getByRole('button', { name: 'Name' }))
  return screen.findByRole('menu')
}

afterEach(cleanup)

describe('DataGridColumnHeader', () => {
  it('updates menu translations and the title with unchanged table state', async () => {
    // A separate table owner keeps the table and seeded order stable while
    // presentation props change, so neither can mask a stale menu memo.
    const { result } = renderHook(() =>
      useTable({ features: dataGridFeatures, data, columns, initialState }),
    )
    const table = result.current
    const column = table.getColumn('name')!
    const { rerender } = render(
      <DataGrid table={table} recordCount={1} tableLayout={menuLayout}>
        <DataGridColumnHeader column={column} title="Name" visibility />
      </DataGrid>,
    )

    rerender(
      <DataGrid
        table={table}
        recordCount={1}
        tableLayout={menuLayout}
        i18n={{
          labels: {
            sortAscending: 'Increasing',
            sortDescending: 'Decreasing',
            pinColumnStart: 'Pin at start',
            pinColumnEnd: 'Pin at end',
            moveColumnStart: 'Move toward start',
            moveColumnEnd: 'Move toward end',
            columnsMenu: 'Choose columns',
            unpinColumn: (title) => `Release ${title}`,
          },
        }}
      >
        <DataGridColumnHeader column={column} title="Contact" visibility />
      </DataGrid>,
    )

    expect(screen.getByRole('button', { name: 'Release Contact' })).toBeInTheDocument()
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Contact' }))
    expect(await screen.findByRole('menuitem', { name: 'Increasing' })).toBeVisible()
    for (const label of [
      'Decreasing',
      'Pin at start',
      'Pin at end',
      'Move toward start',
      'Move toward end',
      'Choose columns',
    ]) {
      expect(screen.getByRole('menuitem', { name: label })).toBeVisible()
    }
    expect(screen.queryByRole('menuitem', { name: 'Asc' })).not.toBeInTheDocument()
  })

  it('cycles sorting from the header when no menu controls are enabled', () => {
    render(<HeaderHarness />)
    const header = screen.getByRole('button', { name: 'Name' })
    expect(header).not.toHaveAttribute('aria-haspopup')

    fireEvent.click(header)
    expect(screen.getByLabelText('Sorting')).toHaveTextContent('[{"id":"name","desc":false}]')
    fireEvent.click(header)
    expect(screen.getByLabelText('Sorting')).toHaveTextContent('[{"id":"name","desc":true}]')
    fireEvent.click(header)
    expect(screen.getByLabelText('Sorting')).toHaveTextContent('[]')
  })

  it('sorts, pins, and moves columns from the menu and blocks movement while pinned', async () => {
    render(<HeaderHarness tableLayout={menuLayout} />)
    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Desc' }))
    expect(screen.getByLabelText('Sorting')).toHaveTextContent('[{"id":"name","desc":true}]')

    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Desc' }))
    expect(screen.getByLabelText('Sorting')).toHaveTextContent('[]')

    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Asc' }))
    expect(screen.getByLabelText('Sorting')).toHaveTextContent('[{"id":"name","desc":false}]')

    await openMenu()
    expect(screen.getByRole('menuitem', { name: 'Move to left' })).toHaveAttribute(
      'aria-disabled',
      'true',
    )
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move to right' }))
    expect(screen.getByLabelText('Column order')).toHaveTextContent('email,name')

    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Pin to left' }))
    expect(screen.getByLabelText('Pinned columns')).toHaveTextContent('{"start":["name"],"end":[]}')
    await openMenu()
    expect(screen.getByRole('menuitem', { name: 'Move to left' })).toHaveAttribute(
      'aria-disabled',
      'true',
    )
    expect(screen.getByRole('menuitem', { name: 'Move to right' })).toHaveAttribute(
      'aria-disabled',
      'true',
    )
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    fireEvent.click(screen.getByRole('button', { name: 'Unpin Name column' }))
    expect(screen.getByLabelText('Pinned columns')).toHaveTextContent('{"start":[],"end":[]}')

    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move to left' }))
    expect(screen.getByLabelText('Column order')).toHaveTextContent('name,email')
    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Pin to right' }))
    expect(screen.getByLabelText('Pinned columns')).toHaveTextContent('{"start":[],"end":["name"]}')
  })

  it('updates column visibility and checkbox state without closing the submenu', async () => {
    render(<HeaderHarness tableLayout={menuLayout} />)
    await openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Columns' }))
    const email = await screen.findByRole('menuitemcheckbox', { name: 'Email' })
    expect(email).toHaveAttribute('aria-checked', 'true')

    fireEvent.click(email)
    expect(screen.getByLabelText('Visible columns')).toHaveTextContent(/^name$/)
    expect(screen.getByRole('menuitemcheckbox', { name: 'Email' })).toHaveAttribute(
      'aria-checked',
      'false',
    )
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Email' }))
    expect(screen.getByLabelText('Visible columns')).toHaveTextContent('name,email')
    expect(screen.getByRole('menuitemcheckbox', { name: 'Email' })).toHaveAttribute(
      'aria-checked',
      'true',
    )
  })

  it.each([
    {
      tableLayout: { columnsPinnable: true, columnsVisibility: true },
      visibility: false,
      canPin: false,
    },
    {
      tableLayout: { columnsPinnable: false, columnsVisibility: false },
      visibility: true,
      canPin: true,
    },
  ])('requires each control gate while retaining the filter section (%j)', async (props) => {
    render(<HeaderHarness {...props} canSort={false} filter />)
    await openMenu()
    expect(screen.getByText('Filter options')).toBeVisible()
    for (const name of [
      'Asc',
      'Desc',
      'Pin to left',
      'Pin to right',
      'Move to left',
      'Move to right',
      'Columns',
    ]) {
      expect(screen.queryByRole('menuitem', { name })).not.toBeInTheDocument()
    }
  })
})
